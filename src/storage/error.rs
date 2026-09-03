use std::{error::Error, fmt, io, path::PathBuf};

use crate::rules::RuleValidationError;

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
    /// A rule violates its model invariants.
    InvalidRule(RuleValidationError),
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
            Self::InvalidRule(error) => write!(formatter, "invalid user rule: {error}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateDataDirectory { source, .. } => Some(source),
            Self::Database(error) => Some(error),
            Self::InvalidRule(error) => Some(error),
            Self::DataDirectoryUnavailable
            | Self::UnsupportedSchemaVersion { .. }
            | Self::InvalidStoredAction(_) => None,
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
