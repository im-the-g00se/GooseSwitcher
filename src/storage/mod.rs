//! SQLite-backed user settings and dictionary rule storage.

mod error;
mod migrations;

use std::{fs, path::Path};

use directories::BaseDirs;
use rusqlite::{params, Connection, OptionalExtension};

use crate::rules::{NewUserRule, RuleAction, UserRule};

pub use error::StorageError;
use migrations::{LATEST_SCHEMA_VERSION, MIGRATION_1};

const APPLICATION_DIRECTORY: &str = "gooseswitcher";
const DATABASE_FILE: &str = "gooseswitcher.db";

/// SQLite-backed storage for GooseSwitcher user data.
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

struct StoredRule {
    id: i64,
    source: String,
    action: String,
    replacement: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<StoredRule> for UserRule {
    type Error = StorageError;

    fn try_from(value: StoredRule) -> Result<Self, Self::Error> {
        let action =
            RuleAction::from_db_value(&value.action).map_err(StorageError::InvalidStoredAction)?;
        Ok(Self {
            id: value.id,
            source: value.source,
            action,
            replacement: value.replacement,
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

    /// Creates or updates a user dictionary rule.
    pub fn upsert_rule(&mut self, rule: NewUserRule) -> Result<UserRule, StorageError> {
        let source = rule.source().to_owned();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO user_rules
                 (source, action, replacement, created_at, updated_at)
             VALUES
                 (?1, ?2, ?3, CAST(strftime('%s', 'now') AS INTEGER),
                  CAST(strftime('%s', 'now') AS INTEGER))
             ON CONFLICT(source) DO UPDATE SET
                 action = excluded.action,
                 replacement = excluded.replacement,
                 updated_at = excluded.updated_at",
            params![
                rule.source(),
                rule.action().as_db_value(),
                rule.replacement()
            ],
        )?;
        transaction.commit()?;

        self.get_rule(&source)?
            .ok_or_else(|| StorageError::Database(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Returns the rule for `source` when one exists.
    pub fn get_rule(&self, source: &str) -> Result<Option<UserRule>, StorageError> {
        let stored = self
            .connection
            .query_row(
                "SELECT id, source, action, replacement, created_at, updated_at
                 FROM user_rules WHERE source = ?1",
                [source],
                stored_rule,
            )
            .optional()?;

        stored.map(UserRule::try_from).transpose()
    }

    /// Returns all user rules ordered by their source token.
    pub fn list_rules(&self) -> Result<Vec<UserRule>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, source, action, replacement, created_at, updated_at
             FROM user_rules ORDER BY source ASC",
        )?;
        let stored = statement
            .query_map([], stored_rule)?
            .collect::<Result<Vec<_>, _>>()?;

        stored.into_iter().map(UserRule::try_from).collect()
    }

    /// Deletes the rule for `source` and reports whether it existed.
    pub fn delete_rule(&mut self, source: &str) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .execute("DELETE FROM user_rules WHERE source = ?1", [source])?
            == 1)
    }
}

fn stored_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRule> {
    Ok(StoredRule {
        id: row.get(0)?,
        source: row.get(1)?,
        action: row.get(2)?,
        replacement: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: version,
            latest: LATEST_SCHEMA_VERSION,
        });
    }

    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
        transaction.commit()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
