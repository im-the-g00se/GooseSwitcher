//! SQLite-backed user settings and dictionary rule storage.

mod error;
mod migrations;

use std::{collections::HashSet, fs, path::Path};

use directories::BaseDirs;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::rules::{
    language_of_word, normalize_word, Language, NewUserRule, RuleAction, RuleValidationError,
    UserRule,
};

pub use error::StorageError;
use migrations::{LATEST_SCHEMA_VERSION, MIGRATION_1, MIGRATION_2_CREATE, MIGRATION_2_FINISH};

const APPLICATION_DIRECTORY: &str = "gooseswitcher";
const DATABASE_FILE: &str = "gooseswitcher.db";
const DEFAULT_SEARCH_LIMIT: usize = 100;
const MAX_SEARCH_LIMIT: usize = 500;

/// SQLite-backed storage for GooseSwitcher user data.
#[derive(Debug)]
pub struct SqliteStore {
    connection: Connection,
}

/// A configuration value stored in SQLite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    /// Stable configuration key.
    pub key: String,
    /// Serialized configuration value.
    pub value: String,
    /// Last update time as Unix seconds in UTC.
    pub updated_at: i64,
}

/// Filters and pagination for user-rule search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSearch {
    /// Case-insensitive substring matched against trigger words.
    pub text: Option<String>,
    /// Optional language filter.
    pub language: Option<Language>,
    /// Optional action filter.
    pub action: Option<RuleAction>,
    /// Maximum number of returned rows, from 1 through 500.
    pub limit: usize,
    /// Number of matching rows to skip.
    pub offset: usize,
}

impl Default for RuleSearch {
    fn default() -> Self {
        Self {
            text: None,
            language: None,
            action: None,
            limit: DEFAULT_SEARCH_LIMIT,
            offset: 0,
        }
    }
}

struct StoredRule {
    id: i64,
    word: String,
    normalized_word: String,
    language: String,
    action: String,
    replacement: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<StoredRule> for UserRule {
    type Error = StorageError;

    fn try_from(value: StoredRule) -> Result<Self, Self::Error> {
        let language = Language::from_db_value(&value.language)
            .map_err(StorageError::InvalidStoredLanguage)?;
        let action =
            RuleAction::from_db_value(&value.action).map_err(StorageError::InvalidStoredAction)?;
        let word = value.word.clone();
        let validated = NewUserRule::from_parts(value.word, language, action, value.replacement)?;
        if validated.normalized_word() != value.normalized_word {
            return Err(StorageError::InvalidStoredNormalizedWord {
                word,
                expected: validated.normalized_word().to_owned(),
                found: value.normalized_word,
            });
        }
        Ok(Self {
            id: value.id,
            word: validated.word().to_owned(),
            normalized_word: value.normalized_word,
            language,
            action,
            replacement: validated.replacement().map(str::to_owned),
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl SqliteStore {
    /// Opens the database in the user's XDG data directory.
    pub fn open_default() -> Result<Self, StorageError> {
        let base_directories = BaseDirs::new().ok_or(StorageError::DataDirectoryUnavailable)?;
        let data_directory = base_directories.data_dir().join(APPLICATION_DIRECTORY);
        fs::create_dir_all(&data_directory).map_err(|source| {
            StorageError::CreateDataDirectory {
                path: data_directory.clone(),
                source,
            }
        })?;
        Self::open(data_directory.join(DATABASE_FILE))
    }

    /// Opens a database at an explicit path and applies pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Returns the setting stored under `key`.
    pub fn get_setting(&self, key: &str) -> Result<Option<Setting>, StorageError> {
        self.connection
            .query_row(
                "SELECT key, value, updated_at FROM settings WHERE key = ?1",
                [key],
                |row| {
                    Ok(Setting {
                        key: row.get(0)?,
                        value: row.get(1)?,
                        updated_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// Creates or updates a setting.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO settings (key, value, updated_at)
             VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER))
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 updated_at = excluded.updated_at",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes a setting and reports whether it existed.
    pub fn delete_setting(&mut self, key: &str) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .execute("DELETE FROM settings WHERE key = ?1", [key])?
            == 1)
    }

    /// Creates a user rule, rejecting an existing normalized key.
    pub fn create_rule(&mut self, rule: NewUserRule) -> Result<UserRule, StorageError> {
        if self
            .find_rule(rule.language(), rule.normalized_word())?
            .is_some()
        {
            return Err(duplicate_error(&rule));
        }
        self.connection.execute(
            "INSERT INTO user_rules
             (word, normalized_word, language, action, replacement, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     CAST(strftime('%s', 'now') AS INTEGER),
                     CAST(strftime('%s', 'now') AS INTEGER))",
            params![
                rule.word(),
                rule.normalized_word(),
                rule.language().as_db_value(),
                rule.action().as_db_value(),
                rule.replacement()
            ],
        )?;
        let id = self.connection.last_insert_rowid();
        self.get_rule(id)?.ok_or_else(query_returned_no_rows)
    }

    /// Replaces the contents of an existing rule while preserving its identity.
    pub fn update_rule(
        &mut self,
        id: i64,
        rule: NewUserRule,
    ) -> Result<Option<UserRule>, StorageError> {
        if let Some(existing) = self.find_rule(rule.language(), rule.normalized_word())? {
            if existing.id != id {
                return Err(duplicate_error(&rule));
            }
        }
        let changed = self.connection.execute(
            "UPDATE user_rules SET
                 word = ?1, normalized_word = ?2, language = ?3,
                 action = ?4, replacement = ?5,
                 updated_at = CAST(strftime('%s', 'now') AS INTEGER)
             WHERE id = ?6",
            params![
                rule.word(),
                rule.normalized_word(),
                rule.language().as_db_value(),
                rule.action().as_db_value(),
                rule.replacement(),
                id
            ],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_rule(id)
    }

    /// Returns a rule by stable database ID.
    pub fn get_rule(&self, id: i64) -> Result<Option<UserRule>, StorageError> {
        select_rule_by_id(&self.connection, id)
    }

    /// Finds a rule by language and case-insensitive word key.
    pub fn find_rule(
        &self,
        language: Language,
        word: &str,
    ) -> Result<Option<UserRule>, StorageError> {
        select_rule_by_key(&self.connection, language, &normalize_word(word))
    }

    /// Searches rules using optional filters and stable pagination.
    pub fn search_rules(&self, search: &RuleSearch) -> Result<Vec<UserRule>, StorageError> {
        if !(1..=MAX_SEARCH_LIMIT).contains(&search.limit) {
            return Err(StorageError::InvalidSearchLimit {
                found: search.limit,
            });
        }
        let normalized_text = search.text.as_deref().map(normalize_word);
        let language = search.language.map(Language::as_db_value);
        let action = search.action.map(RuleAction::as_db_value);
        let offset =
            i64::try_from(search.offset).map_err(|_| StorageError::InvalidSearchOffset {
                found: search.offset,
            })?;
        let mut statement = self.connection.prepare(
            "SELECT id, word, normalized_word, language, action, replacement,
                    created_at, updated_at
             FROM user_rules
             WHERE (?1 IS NULL OR instr(normalized_word, ?1) > 0)
               AND (?2 IS NULL OR language = ?2)
               AND (?3 IS NULL OR action = ?3)
             ORDER BY normalized_word ASC, language ASC, id ASC
             LIMIT ?4 OFFSET ?5",
        )?;
        let stored = statement
            .query_map(
                params![
                    normalized_text,
                    language,
                    action,
                    search.limit as i64,
                    offset
                ],
                stored_rule,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        stored.into_iter().map(UserRule::try_from).collect()
    }

    /// Deletes a rule by ID and reports whether it existed.
    pub fn delete_rule(&mut self, id: i64) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .execute("DELETE FROM user_rules WHERE id = ?1", [id])?
            == 1)
    }

    pub(crate) fn all_rules(&self) -> Result<Vec<UserRule>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, word, normalized_word, language, action, replacement,
                    created_at, updated_at
             FROM user_rules
             ORDER BY normalized_word ASC, language ASC, id ASC",
        )?;
        let stored = statement
            .query_map([], stored_rule)?
            .collect::<Result<Vec<_>, _>>()?;
        stored.into_iter().map(UserRule::try_from).collect()
    }

    pub(crate) fn transaction(&mut self) -> Result<Transaction<'_>, StorageError> {
        self.connection.transaction().map_err(StorageError::from)
    }
}

fn duplicate_error(rule: &NewUserRule) -> StorageError {
    StorageError::DuplicateRule {
        language: rule.language(),
        normalized_word: rule.normalized_word().to_owned(),
    }
}

fn query_returned_no_rows() -> StorageError {
    StorageError::Database(rusqlite::Error::QueryReturnedNoRows)
}

pub(crate) fn select_rule_by_id(
    connection: &Connection,
    id: i64,
) -> Result<Option<UserRule>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT id, word, normalized_word, language, action, replacement,
                    created_at, updated_at
             FROM user_rules WHERE id = ?1",
            [id],
            stored_rule,
        )
        .optional()?;
    stored.map(UserRule::try_from).transpose()
}

pub(crate) fn select_rule_by_key(
    connection: &Connection,
    language: Language,
    normalized_word: &str,
) -> Result<Option<UserRule>, StorageError> {
    let stored = connection
        .query_row(
            "SELECT id, word, normalized_word, language, action, replacement,
                    created_at, updated_at
             FROM user_rules WHERE language = ?1 AND normalized_word = ?2",
            params![language.as_db_value(), normalized_word],
            stored_rule,
        )
        .optional()?;
    stored.map(UserRule::try_from).transpose()
}

fn stored_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRule> {
    Ok(StoredRule {
        id: row.get(0)?,
        word: row.get(1)?,
        normalized_word: row.get(2)?,
        language: row.get(3)?,
        action: row.get(4)?,
        replacement: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let mut version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: version,
            latest: LATEST_SCHEMA_VERSION,
        });
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
        version = 1;
    }
    if version == 1 {
        migrate_version_one(connection)?;
    }
    Ok(())
}

fn migrate_version_one(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(MIGRATION_2_CREATE)?;
    let legacy = {
        let mut statement = transaction.prepare(
            "SELECT id, source, action, replacement, created_at, updated_at
             FROM user_rules ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut keys = HashSet::new();
    for (id, word, action, replacement, created_at, updated_at) in legacy {
        let action =
            RuleAction::from_db_value(&action).map_err(StorageError::InvalidStoredAction)?;
        let language =
            infer_legacy_language(&word).map_err(|source| StorageError::MigrationRule {
                id,
                word: word.clone(),
                source,
            })?;
        let rule = NewUserRule::from_parts(word.clone(), language, action, replacement).map_err(
            |source| StorageError::MigrationRule {
                id,
                word: word.clone(),
                source,
            },
        )?;
        let key = (language, rule.normalized_word().to_owned());
        if !keys.insert(key.clone()) {
            return Err(StorageError::MigrationConflict {
                language: key.0,
                normalized_word: key.1,
            });
        }
        transaction.execute(
            "INSERT INTO user_rules_v2
             (id, word, normalized_word, language, action, replacement,
              created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                rule.word(),
                rule.normalized_word(),
                language.as_db_value(),
                action.as_db_value(),
                rule.replacement(),
                created_at,
                updated_at
            ],
        )?;
    }
    transaction.execute_batch(MIGRATION_2_FINISH)?;
    transaction.pragma_update(None, "user_version", 2)?;
    transaction.commit()?;
    Ok(())
}

fn infer_legacy_language(word: &str) -> Result<Language, RuleValidationError> {
    if let Some(language) = language_of_word(word) {
        return Ok(language);
    }
    let language = word
        .chars()
        .find_map(|character| {
            if character.is_ascii_alphabetic() {
                Some(Language::English)
            } else if matches!(character, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё') {
                Some(Language::Russian)
            } else {
                None
            }
        })
        .unwrap_or(Language::English);
    NewUserRule::consider_correct(word, language).map(|_| language)
}

#[cfg(test)]
mod tests;
