use std::{error::Error, fmt, io, path::PathBuf};

use crate::rules::{Language, RuleValidationError};

/// Error returned by SQLite storage operations.
#[derive(Debug)]
pub enum StorageError {
    /// The user's data directory could not be determined.
    DataDirectoryUnavailable,
    /// The application data directory could not be created.
    CreateDataDirectory { path: PathBuf, source: io::Error },
    /// SQLite rejected an operation.
    Database(rusqlite::Error),
    /// The database was created by a newer, unsupported application version.
    UnsupportedSchemaVersion { found: i64, latest: i64 },
    /// A stored rule contains an unknown action value.
    InvalidStoredAction(String),
    /// A stored rule contains an unknown language value.
    InvalidStoredLanguage(String),
    /// A stored lookup key does not match the normalized displayed word.
    InvalidStoredNormalizedWord {
        word: String,
        expected: String,
        found: String,
    },
    /// A rule violates its model invariants.
    InvalidRule(RuleValidationError),
    /// A legacy rule cannot be represented in the current model.
    MigrationRule {
        id: i64,
        word: String,
        source: RuleValidationError,
    },
    /// Legacy rules collapse to the same normalized key.
    MigrationConflict {
        language: Language,
        normalized_word: String,
    },
    /// The normalized rule key already exists.
    DuplicateRule {
        language: Language,
        normalized_word: String,
    },
    /// Search page size is outside 1..=500.
    InvalidSearchLimit { found: usize },
    /// Search offset cannot be represented by SQLite's signed integer.
    InvalidSearchOffset { found: usize },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirectoryUnavailable => {
                formatter.write_str("user data directory is unavailable")
            }
            Self::CreateDataDirectory { path, .. } => {
                write!(
                    formatter,
                    "failed to create data directory {}",
                    path.display()
                )
            }
            Self::Database(error) => write!(formatter, "SQLite error: {error}"),
            Self::UnsupportedSchemaVersion { found, latest } => write!(
                formatter,
                "database schema version {found} is newer than supported version {latest}"
            ),
            Self::InvalidStoredAction(action) => {
                write!(
                    formatter,
                    "database contains unknown rule action {action:?}"
                )
            }
            Self::InvalidStoredLanguage(language) => {
                write!(
                    formatter,
                    "database contains unknown rule language {language:?}"
                )
            }
            Self::InvalidStoredNormalizedWord {
                word,
                expected,
                found,
            } => write!(
                formatter,
                "stored normalized key {found:?} for {word:?} should be {expected:?}"
            ),
            Self::InvalidRule(error) => write!(formatter, "invalid user rule: {error}"),
            Self::MigrationRule { id, word, .. } => {
                write!(formatter, "cannot migrate user rule {id} ({word:?})")
            }
            Self::MigrationConflict {
                language,
                normalized_word,
            } => write!(
                formatter,
                "legacy rules conflict at {language:?}/{normalized_word:?}"
            ),
            Self::DuplicateRule {
                language,
                normalized_word,
            } => write!(
                formatter,
                "user rule already exists at {language:?}/{normalized_word:?}"
            ),
            Self::InvalidSearchLimit { found } => {
                write!(formatter, "search limit {found} is outside 1..=500")
            }
            Self::InvalidSearchOffset { found } => {
                write!(formatter, "search offset {found} exceeds SQLite's range")
            }
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateDataDirectory { source, .. } => Some(source),
            Self::Database(error) => Some(error),
            Self::InvalidRule(error) => Some(error),
            Self::MigrationRule { source, .. } => Some(source),
            Self::DataDirectoryUnavailable
            | Self::UnsupportedSchemaVersion { .. }
            | Self::InvalidStoredAction(_)
            | Self::InvalidStoredLanguage(_)
            | Self::InvalidStoredNormalizedWord { .. }
            | Self::MigrationConflict { .. }
            | Self::DuplicateRule { .. }
            | Self::InvalidSearchLimit { .. }
            | Self::InvalidSearchOffset { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<RuleValidationError> for StorageError {
    fn from(error: RuleValidationError) -> Self {
        Self::InvalidRule(error)
    }
}
