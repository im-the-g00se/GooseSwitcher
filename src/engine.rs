//! Startup-owned recognition state with no storage access in the hot path.

use std::{collections::HashMap, error::Error, fmt};

use crate::{
    config::{CORRECTIONS_ENABLED, MINIMUM_WORD_LENGTH},
    dictionaries::{DictionaryLoadError, SystemDictionaryIndex, SystemDictionaryPaths},
    recognition::{decide_correction, CorrectionDecision, RecognitionConfig},
    rules::UserRule,
    storage::{SqliteStore, StorageError},
};

/// Dictionaries, rules and settings snapshotted once during engine startup.
pub struct RecognitionRuntime {
    dictionaries: SystemDictionaryIndex,
    user_rules: HashMap<String, UserRule>,
    config: RecognitionConfig,
}

impl RecognitionRuntime {
    /// Loads all state required for per-word decisions.
    pub fn load(
        store: &SqliteStore,
        dictionary_paths: &SystemDictionaryPaths,
    ) -> Result<Self, RuntimeLoadError> {
        let dictionaries = SystemDictionaryIndex::load(dictionary_paths)?;
        let config = load_config(store)?;
        let user_rules = store
            .list_rules()?
            .into_iter()
            .map(|rule| (rule.source.clone(), rule))
            .collect();
        Ok(Self {
            dictionaries,
            user_rules,
            config,
        })
    }

    /// Evaluates one completed word using only state held in memory.
    pub fn decide(&self, source: &str) -> CorrectionDecision {
        if !self.config.corrections_enabled {
            return CorrectionDecision::Keep;
        }

        let matching_rule = self.user_rules.get(source);
        decide_correction(
            source,
            self.dictionaries.dictionaries(),
            matching_rule,
            self.config,
        )
    }

    /// Returns the startup settings snapshot.
    pub fn config(&self) -> RecognitionConfig {
        self.config
    }
}

fn load_config(store: &SqliteStore) -> Result<RecognitionConfig, RuntimeLoadError> {
    let mut config = RecognitionConfig::default();

    if let Some(setting) = store.get_setting(CORRECTIONS_ENABLED)? {
        config.corrections_enabled = match setting.value.as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(RuntimeLoadError::InvalidSetting(setting)),
        };
    }

    if let Some(setting) = store.get_setting(MINIMUM_WORD_LENGTH)? {
        config.minimum_word_length = setting
            .value
            .parse()
            .map_err(|_| RuntimeLoadError::InvalidSetting(setting))?;
    }

    Ok(config)
}

/// Failure while building the in-memory recognition runtime.
#[derive(Debug)]
pub enum RuntimeLoadError {
    /// A system dictionary could not be loaded.
    Dictionary(DictionaryLoadError),
    /// Startup state could not be read from SQLite.
    Storage(StorageError),
    /// A recognized setting has an invalid serialized value.
    InvalidSetting(crate::storage::Setting),
}

impl fmt::Display for RuntimeLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dictionary(error) => error.fmt(formatter),
            Self::Storage(error) => error.fmt(formatter),
            Self::InvalidSetting(setting) => write!(
                formatter,
                "invalid value {:?} for setting {:?}",
                setting.value, setting.key
            ),
        }
    }
}

impl Error for RuntimeLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Dictionary(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::InvalidSetting(_) => None,
        }
    }
}

impl From<DictionaryLoadError> for RuntimeLoadError {
    fn from(error: DictionaryLoadError) -> Self {
        Self::Dictionary(error)
    }
}

impl From<StorageError> for RuntimeLoadError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}
