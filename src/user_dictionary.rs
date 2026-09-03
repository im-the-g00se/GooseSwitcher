//! Portable JSON import and export for user dictionary rules.

use std::{
    collections::HashSet,
    error::Error,
    fmt,
    io::{self, Read, Write},
};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    rules::{Language, NewUserRule, RuleAction, RuleValidationError},
    storage::{select_rule_by_key, SqliteStore, StorageError},
};

const FORMAT_NAME: &str = "gooseswitcher-user-dictionary";
const FORMAT_VERSION: u32 = 1;
pub(crate) const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_RULES: usize = 10_000;

/// Behavior when an imported rule has the same normalized key as stored data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportConflictPolicy {
    /// Reject the entire import without changing the database.
    #[default]
    Reject,
    /// Preserve the stored rule and continue importing other entries.
    Skip,
    /// Replace stored rule content while preserving its identity and creation time.
    Replace,
}

/// Counts of changes made by a successful import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Newly created rules.
    pub created: usize,
    /// Existing rules replaced by imported content.
    pub updated: usize,
    /// Existing rules preserved by [`ImportConflictPolicy::Skip`].
    pub skipped: usize,
}

/// Error returned by user-dictionary JSON import or export.
#[derive(Debug)]
pub enum DictionaryTransferError {
    /// Reading or writing the document failed.
    Io(io::Error),
    /// The document is not valid strict JSON for this format.
    Json(serde_json::Error),
    /// The document identifies a different format.
    InvalidFormat(String),
    /// The document format version is unsupported.
    UnsupportedVersion(u32),
    /// The document exceeds the byte limit.
    DocumentTooLarge { max_bytes: usize },
    /// The document contains too many rules.
    TooManyRules { max_rules: usize },
    /// A rule contains an unsupported language value.
    InvalidLanguage { index: usize, value: String },
    /// A rule contains an unsupported action value.
    InvalidAction { index: usize, value: String },
    /// A rule violates model validation.
    InvalidRule {
        index: usize,
        source: RuleValidationError,
    },
    /// Two entries in the document have the same normalized key.
    DuplicateInDocument {
        language: Language,
        normalized_word: String,
    },
    /// An imported key already exists and the policy is reject.
    Conflict {
        language: Language,
        normalized_word: String,
    },
    /// SQLite storage failed.
    Storage(StorageError),
}

impl fmt::Display for DictionaryTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "dictionary I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid dictionary JSON: {error}"),
            Self::InvalidFormat(value) => {
                write!(formatter, "unsupported dictionary format {value:?}")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported dictionary format version {version}")
            }
            Self::DocumentTooLarge { max_bytes } => {
                write!(formatter, "dictionary document exceeds {max_bytes} bytes")
            }
            Self::TooManyRules { max_rules } => {
                write!(formatter, "dictionary contains more than {max_rules} rules")
            }
            Self::InvalidLanguage { index, value } => {
                write!(formatter, "rule {index} has invalid language {value:?}")
            }
            Self::InvalidAction { index, value } => {
                write!(formatter, "rule {index} has invalid type {value:?}")
            }
            Self::InvalidRule { index, .. } => write!(formatter, "rule {index} is invalid"),
            Self::DuplicateInDocument {
                language,
                normalized_word,
            } => write!(
                formatter,
                "dictionary repeats {language:?}/{normalized_word:?}"
            ),
            Self::Conflict {
                language,
                normalized_word,
            } => write!(
                formatter,
                "stored rule conflicts at {language:?}/{normalized_word:?}"
            ),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl Error for DictionaryTransferError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidRule { source, .. } => Some(source),
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StorageError> for DictionaryTransferError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<rusqlite::Error> for DictionaryTransferError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(StorageError::from(error))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportDocument {
    format: String,
    version: u32,
    rules: Vec<ImportRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRule {
    word: String,
    language: String,
    #[serde(rename = "type")]
    action: String,
    replacement: Option<String>,
}

#[derive(Serialize)]
struct ExportDocument<'a> {
    format: &'static str,
    version: u32,
    rules: Vec<ExportRule<'a>>,
}

#[derive(Serialize)]
struct ExportRule<'a> {
    word: &'a str,
    language: &'static str,
    #[serde(rename = "type")]
    action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    replacement: Option<&'a str>,
}

/// Writes all user rules as deterministic, pretty-printed UTF-8 JSON.
pub fn export_json<W: Write>(
    store: &SqliteStore,
    mut writer: W,
) -> Result<(), DictionaryTransferError> {
    let rules = store.all_rules()?;
    let document = ExportDocument {
        format: FORMAT_NAME,
        version: FORMAT_VERSION,
        rules: rules
            .iter()
            .map(|rule| ExportRule {
                word: &rule.word,
                language: rule.language.as_db_value(),
                action: rule.action.as_db_value(),
                replacement: rule.replacement.as_deref(),
            })
            .collect(),
    };
    let mut json = serde_json::to_vec_pretty(&document).map_err(DictionaryTransferError::Json)?;
    json.push(b'\n');
    writer.write_all(&json).map_err(DictionaryTransferError::Io)
}

/// Imports a strict versioned JSON document in one SQLite transaction.
pub fn import_json<R: Read>(
    store: &mut SqliteStore,
    reader: R,
    policy: ImportConflictPolicy,
) -> Result<ImportReport, DictionaryTransferError> {
    let rules = parse_document(reader)?;
    let transaction = store.transaction()?;
    let mut report = ImportReport::default();

    for rule in rules {
        let existing = select_rule_by_key(&transaction, rule.language(), rule.normalized_word())?;
        match (existing, policy) {
            (Some(_), ImportConflictPolicy::Reject) => {
                return Err(DictionaryTransferError::Conflict {
                    language: rule.language(),
                    normalized_word: rule.normalized_word().to_owned(),
                });
            }
            (Some(_), ImportConflictPolicy::Skip) => report.skipped += 1,
            (Some(existing), ImportConflictPolicy::Replace) => {
                transaction.execute(
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
                        existing.id
                    ],
                )?;
                report.updated += 1;
            }
            (None, _) => {
                transaction.execute(
                    "INSERT INTO user_rules
                     (word, normalized_word, language, action, replacement,
                      created_at, updated_at)
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
                report.created += 1;
            }
        }
    }
    transaction.commit().map_err(StorageError::from)?;
    Ok(report)
}

fn parse_document<R: Read>(reader: R) -> Result<Vec<NewUserRule>, DictionaryTransferError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(DictionaryTransferError::Io)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(DictionaryTransferError::DocumentTooLarge {
            max_bytes: MAX_DOCUMENT_BYTES,
        });
    }
    let document: ImportDocument =
        serde_json::from_slice(&bytes).map_err(DictionaryTransferError::Json)?;
    if document.format != FORMAT_NAME {
        return Err(DictionaryTransferError::InvalidFormat(document.format));
    }
    if document.version != FORMAT_VERSION {
        return Err(DictionaryTransferError::UnsupportedVersion(
            document.version,
        ));
    }
    if document.rules.len() > MAX_RULES {
        return Err(DictionaryTransferError::TooManyRules {
            max_rules: MAX_RULES,
        });
    }

    let mut keys = HashSet::with_capacity(document.rules.len());
    let mut rules = Vec::with_capacity(document.rules.len());
    for (index, raw) in document.rules.into_iter().enumerate() {
        let language = Language::from_db_value(&raw.language)
            .map_err(|value| DictionaryTransferError::InvalidLanguage { index, value })?;
        let action = RuleAction::from_db_value(&raw.action)
            .map_err(|value| DictionaryTransferError::InvalidAction { index, value })?;
        let rule = NewUserRule::from_parts(raw.word, language, action, raw.replacement)
            .map_err(|source| DictionaryTransferError::InvalidRule { index, source })?;
        let key = (language, rule.normalized_word().to_owned());
        if !keys.insert(key.clone()) {
            return Err(DictionaryTransferError::DuplicateInDocument {
                language: key.0,
                normalized_word: key.1,
            });
        }
        rules.push(rule);
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use crate::{
        rules::{Language, NewUserRule, RuleAction},
        storage::{RuleSearch, SqliteStore},
    };

    use super::*;

    fn store() -> (tempfile::TempDir, SqliteStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(directory.path().join("dictionary.db")).unwrap();
        (directory, store)
    }

    #[test]
    fn every_rule_type_round_trips_through_stable_json() {
        let (_directory, mut source) = store();
        source
            .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
            .unwrap();
        source
            .create_rule(
                NewUserRule::always_replace("ghbdtn", Language::English, "привет").unwrap(),
            )
            .unwrap();
        source
            .create_rule(NewUserRule::never_correct("ёжик", Language::Russian).unwrap())
            .unwrap();

        let mut json = Vec::new();
        export_json(&source, &mut json).unwrap();
        assert_eq!(
            String::from_utf8(json.clone()).unwrap(),
            concat!(
                "{\n",
                "  \"format\": \"gooseswitcher-user-dictionary\",\n",
                "  \"version\": 1,\n",
                "  \"rules\": [\n",
                "    {\n",
                "      \"word\": \"ghbdtn\",\n",
                "      \"language\": \"en\",\n",
                "      \"type\": \"always_replace\",\n",
                "      \"replacement\": \"привет\"\n",
                "    },\n",
                "    {\n",
                "      \"word\": \"Rust\",\n",
                "      \"language\": \"en\",\n",
                "      \"type\": \"consider_correct\"\n",
                "    },\n",
                "    {\n",
                "      \"word\": \"ёжик\",\n",
                "      \"language\": \"ru\",\n",
                "      \"type\": \"never_correct\"\n",
                "    }\n",
                "  ]\n",
                "}\n"
            )
        );

        let (_target_directory, mut target) = store();
        let report =
            import_json(&mut target, json.as_slice(), ImportConflictPolicy::Reject).unwrap();
        assert_eq!(
            report,
            ImportReport {
                created: 3,
                updated: 0,
                skipped: 0,
            }
        );
        let rules = target.search_rules(&RuleSearch::default()).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].action, RuleAction::AlwaysReplace);
        assert_eq!(rules[1].word, "Rust");
        assert_eq!(rules[2].language, Language::Russian);
    }

    #[test]
    fn import_rejects_wrong_envelope_unknown_fields_and_invalid_rules() {
        let cases = [
            r#"{"format":"other","version":1,"rules":[]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":2,"rules":[]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":1,"extra":1,"rules":[]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":1,"rules":[{"word":"hello","language":"de","type":"consider_correct"}]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":1,"rules":[{"word":"hello","language":"en","type":"unknown"}]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":1,"rules":[{"word":"hello","language":"en","type":"never_correct","replacement":"x"}]}"#,
            r#"{"format":"gooseswitcher-user-dictionary","version":1,"rules":[{"word":"привет","language":"en","type":"consider_correct"}]}"#,
        ];

        for json in cases {
            let (_directory, mut store) = store();
            assert!(
                import_json(&mut store, json.as_bytes(), ImportConflictPolicy::Reject).is_err()
            );
            assert!(store
                .search_rules(&RuleSearch::default())
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn import_rejects_normalized_duplicates_within_document() {
        let json = br#"{
          "format":"gooseswitcher-user-dictionary",
          "version":1,
          "rules":[
            {"word":"Rust","language":"en","type":"consider_correct"},
            {"word":"rust","language":"en","type":"never_correct"}
          ]
        }"#;
        let (_directory, mut store) = store();

        let error =
            import_json(&mut store, json.as_slice(), ImportConflictPolicy::Reject).unwrap_err();
        assert!(matches!(
            error,
            DictionaryTransferError::DuplicateInDocument {
                language: Language::English,
                ref normalized_word
            } if normalized_word == "rust"
        ));
    }

    #[test]
    fn reject_policy_leaves_database_unchanged_on_conflict() {
        let (_directory, mut store) = store();
        store
            .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
            .unwrap();
        let json = br#"{
          "format":"gooseswitcher-user-dictionary","version":1,
          "rules":[
            {"word":"alpha","language":"en","type":"consider_correct"},
            {"word":"rust","language":"en","type":"never_correct"}
          ]
        }"#;

        let error =
            import_json(&mut store, json.as_slice(), ImportConflictPolicy::Reject).unwrap_err();
        assert!(matches!(error, DictionaryTransferError::Conflict { .. }));
        assert!(store
            .find_rule(Language::English, "alpha")
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .find_rule(Language::English, "rust")
                .unwrap()
                .unwrap()
                .action,
            RuleAction::ConsiderCorrect
        );
    }

    #[test]
    fn skip_and_replace_policies_report_exact_outcomes() {
        let json = br#"{
          "format":"gooseswitcher-user-dictionary","version":1,
          "rules":[
            {"word":"alpha","language":"en","type":"consider_correct"},
            {"word":"rust","language":"en","type":"never_correct"}
          ]
        }"#;

        let (_skip_directory, mut skip_store) = store();
        skip_store
            .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
            .unwrap();
        assert_eq!(
            import_json(&mut skip_store, json.as_slice(), ImportConflictPolicy::Skip).unwrap(),
            ImportReport {
                created: 1,
                updated: 0,
                skipped: 1
            }
        );

        let (_replace_directory, mut replace_store) = store();
        let original = replace_store
            .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
            .unwrap();
        assert_eq!(
            import_json(
                &mut replace_store,
                json.as_slice(),
                ImportConflictPolicy::Replace
            )
            .unwrap(),
            ImportReport {
                created: 1,
                updated: 1,
                skipped: 0
            }
        );
        let replaced = replace_store
            .find_rule(Language::English, "rust")
            .unwrap()
            .unwrap();
        assert_eq!(replaced.id, original.id);
        assert_eq!(replaced.created_at, original.created_at);
        assert_eq!(replaced.action, RuleAction::NeverCorrect);
    }

    #[test]
    fn import_enforces_document_size_and_rule_count_limits() {
        let (_size_directory, mut size_store) = store();
        let oversized = vec![b' '; MAX_DOCUMENT_BYTES + 1];
        assert!(matches!(
            import_json(
                &mut size_store,
                oversized.as_slice(),
                ImportConflictPolicy::Reject
            ),
            Err(DictionaryTransferError::DocumentTooLarge { .. })
        ));

        let mut rules = Vec::new();
        for index in 0..=MAX_RULES {
            rules.push(format!(
                r#"{{"word":"a{index}","language":"en","type":"consider_correct"}}"#
            ));
        }
        let json = format!(
            r#"{{"format":"gooseswitcher-user-dictionary","version":1,"rules":[{}]}}"#,
            rules.join(",")
        );
        let (_count_directory, mut count_store) = store();
        assert!(matches!(
            import_json(
                &mut count_store,
                json.as_bytes(),
                ImportConflictPolicy::Reject
            ),
            Err(DictionaryTransferError::TooManyRules { .. })
        ));
    }
}
