//! User dictionary rule types and validation.

use std::{error::Error, fmt};

pub(crate) const MAX_WORD_LENGTH: usize = 128;
pub(crate) const MAX_REPLACEMENT_LENGTH: usize = 1024;

/// Language of a user dictionary trigger word.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Language {
    /// Russian Cyrillic letters, including `ё`.
    Russian,
    /// English ASCII letters.
    English,
}

impl Language {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::Russian => "ru",
            Self::English => "en",
        }
    }

    pub(crate) fn from_db_value(value: &str) -> Result<Self, String> {
        match value {
            "ru" => Ok(Self::Russian),
            "en" => Ok(Self::English),
            other => Err(other.to_owned()),
        }
    }
}

/// Action applied to a token by a user dictionary rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleAction {
    /// Treat the token as a valid word.
    ConsiderCorrect,
    /// Never correct the token automatically.
    NeverCorrect,
    /// Always replace the token with the rule's replacement.
    AlwaysReplace,
}

impl RuleAction {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::ConsiderCorrect => "consider_correct",
            Self::NeverCorrect => "never_correct",
            Self::AlwaysReplace => "always_replace",
        }
    }

    pub(crate) fn from_db_value(value: &str) -> Result<Self, String> {
        match value {
            "consider_correct" => Ok(Self::ConsiderCorrect),
            "never_correct" => Ok(Self::NeverCorrect),
            "always_replace" => Ok(Self::AlwaysReplace),
            other => Err(other.to_owned()),
        }
    }
}

/// A validated rule ready to be persisted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewUserRule {
    word: String,
    normalized_word: String,
    language: Language,
    action: RuleAction,
    replacement: Option<String>,
}

impl NewUserRule {
    /// Creates a rule that treats `word` as correct.
    pub fn consider_correct(
        word: impl Into<String>,
        language: Language,
    ) -> Result<Self, RuleValidationError> {
        Self::new(word.into(), language, RuleAction::ConsiderCorrect, None)
    }

    /// Creates a rule that prevents automatic correction of `word`.
    pub fn never_correct(
        word: impl Into<String>,
        language: Language,
    ) -> Result<Self, RuleValidationError> {
        Self::new(word.into(), language, RuleAction::NeverCorrect, None)
    }

    /// Creates a rule that always replaces `word` with `replacement`.
    pub fn always_replace(
        word: impl Into<String>,
        language: Language,
        replacement: impl Into<String>,
    ) -> Result<Self, RuleValidationError> {
        let replacement = replacement.into();
        validate_replacement(&replacement)?;
        Self::new(
            word.into(),
            language,
            RuleAction::AlwaysReplace,
            Some(replacement),
        )
    }

    pub(crate) fn from_parts(
        word: impl Into<String>,
        language: Language,
        action: RuleAction,
        replacement: Option<String>,
    ) -> Result<Self, RuleValidationError> {
        let word = word.into();
        match action {
            RuleAction::ConsiderCorrect | RuleAction::NeverCorrect if replacement.is_some() => {
                return Err(RuleValidationError::UnexpectedReplacement)
            }
            RuleAction::AlwaysReplace => {
                validate_replacement(
                    replacement
                        .as_deref()
                        .ok_or(RuleValidationError::EmptyReplacement)?,
                )?;
            }
            _ => {}
        }
        Self::new(word, language, action, replacement)
    }

    fn new(
        word: String,
        language: Language,
        action: RuleAction,
        replacement: Option<String>,
    ) -> Result<Self, RuleValidationError> {
        validate_word(&word, language)?;
        Ok(Self {
            normalized_word: normalize_word(&word),
            word,
            language,
            action,
            replacement,
        })
    }

    /// Returns the displayed trigger word.
    pub fn word(&self) -> &str {
        &self.word
    }

    /// Returns the case-insensitive lookup key.
    pub fn normalized_word(&self) -> &str {
        &self.normalized_word
    }

    /// Returns the trigger word language.
    pub fn language(&self) -> Language {
        self.language
    }

    /// Returns the action performed by this rule.
    pub fn action(&self) -> RuleAction {
        self.action
    }

    /// Returns replacement text for an always-replace rule.
    pub fn replacement(&self) -> Option<&str> {
        self.replacement.as_deref()
    }
}

/// A user dictionary rule loaded from storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRule {
    /// Stable database identifier.
    pub id: i64,
    /// Displayed trigger word.
    pub word: String,
    /// Case-insensitive lookup key.
    pub normalized_word: String,
    /// Trigger word language.
    pub language: Language,
    /// Action performed by the rule.
    pub action: RuleAction,
    /// Replacement text for [`RuleAction::AlwaysReplace`].
    pub replacement: Option<String>,
    /// Creation time as Unix seconds in UTC.
    pub created_at: i64,
    /// Last update time as Unix seconds in UTC.
    pub updated_at: i64,
}

/// Error returned when a user rule violates its invariants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleValidationError {
    /// The trigger word is empty.
    EmptyWord,
    /// The trigger word exceeds the supported limit.
    WordTooLong { max: usize },
    /// The trigger contains a character that cannot occur in a word.
    InvalidWordCharacter { character: char },
    /// A letter does not belong to the declared language.
    LanguageMismatch { language: Language, character: char },
    /// A joiner occurs at a boundary or next to another joiner.
    InvalidJoiner,
    /// An always-replace rule has no replacement text.
    EmptyReplacement,
    /// Replacement text exceeds the supported limit.
    ReplacementTooLong { max: usize },
    /// Replacement text has leading or trailing whitespace.
    ReplacementBoundaryWhitespace,
    /// Replacement text contains a control character.
    ReplacementControlCharacter,
    /// A non-replacement action unexpectedly carries replacement text.
    UnexpectedReplacement,
}

impl fmt::Display for RuleValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWord => formatter.write_str("rule word must not be empty"),
            Self::WordTooLong { max } => write!(formatter, "rule word exceeds {max} characters"),
            Self::InvalidWordCharacter { character } => {
                write!(formatter, "invalid character {character:?} in rule word")
            }
            Self::LanguageMismatch {
                language,
                character,
            } => write!(
                formatter,
                "character {character:?} does not belong to {language:?}"
            ),
            Self::InvalidJoiner => formatter.write_str("invalid joiner placement in rule word"),
            Self::EmptyReplacement => formatter.write_str("replacement must not be empty"),
            Self::ReplacementTooLong { max } => {
                write!(formatter, "replacement exceeds {max} characters")
            }
            Self::ReplacementBoundaryWhitespace => {
                formatter.write_str("replacement must not have boundary whitespace")
            }
            Self::ReplacementControlCharacter => {
                formatter.write_str("replacement must not contain control characters")
            }
            Self::UnexpectedReplacement => {
                formatter.write_str("replacement is only valid for always-replace rules")
            }
        }
    }
}

impl Error for RuleValidationError {}

pub(crate) fn normalize_word(word: &str) -> String {
    word.chars().flat_map(char::to_lowercase).collect()
}

pub(crate) fn language_of_word(word: &str) -> Option<Language> {
    let mut language = None;
    for character in word.chars() {
        let current = if character.is_ascii_alphabetic() {
            Language::English
        } else if is_russian_letter(character) {
            Language::Russian
        } else if is_joiner(character) {
            continue;
        } else {
            return None;
        };
        if language.is_some_and(|language| language != current) {
            return None;
        }
        language = Some(current);
    }
    language
}

fn validate_word(word: &str, language: Language) -> Result<(), RuleValidationError> {
    let characters: Vec<_> = word.chars().collect();
    if characters.is_empty() {
        return Err(RuleValidationError::EmptyWord);
    }
    if characters.len() > MAX_WORD_LENGTH {
        return Err(RuleValidationError::WordTooLong {
            max: MAX_WORD_LENGTH,
        });
    }

    for (index, &character) in characters.iter().enumerate() {
        if is_joiner(character) {
            if index == 0
                || index + 1 == characters.len()
                || is_joiner(characters[index - 1])
                || is_joiner(characters[index + 1])
            {
                return Err(RuleValidationError::InvalidJoiner);
            }
            continue;
        }

        let matches_language = match language {
            Language::English => character.is_ascii_alphabetic(),
            Language::Russian => is_russian_letter(character),
        };
        if matches_language {
            continue;
        }
        if character.is_ascii_alphabetic() || is_russian_letter(character) {
            return Err(RuleValidationError::LanguageMismatch {
                language,
                character,
            });
        }
        return Err(RuleValidationError::InvalidWordCharacter { character });
    }
    Ok(())
}

fn validate_replacement(replacement: &str) -> Result<(), RuleValidationError> {
    let length = replacement.chars().count();
    if length == 0 {
        return Err(RuleValidationError::EmptyReplacement);
    }
    if length > MAX_REPLACEMENT_LENGTH {
        return Err(RuleValidationError::ReplacementTooLong {
            max: MAX_REPLACEMENT_LENGTH,
        });
    }
    if replacement.chars().next().is_some_and(char::is_whitespace)
        || replacement.chars().last().is_some_and(char::is_whitespace)
    {
        return Err(RuleValidationError::ReplacementBoundaryWhitespace);
    }
    if replacement.chars().any(char::is_control) {
        return Err(RuleValidationError::ReplacementControlCharacter);
    }
    Ok(())
}

fn is_russian_letter(character: char) -> bool {
    matches!(character, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

fn is_joiner(character: char) -> bool {
    matches!(character, '-' | '‐' | '‑' | '\'' | '’')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_words_are_normalized_without_changing_display_spelling() {
        let english = NewUserRule::consider_correct("Rust", Language::English).unwrap();
        let russian = NewUserRule::never_correct("Ёлка-парк", Language::Russian).unwrap();

        assert_eq!(english.word(), "Rust");
        assert_eq!(english.normalized_word(), "rust");
        assert_eq!(english.language(), Language::English);
        assert_eq!(russian.normalized_word(), "ёлка-парк");
    }

    #[test]
    fn word_validation_rejects_invalid_inputs() {
        let too_long = "a".repeat(129);
        let cases = [
            (
                NewUserRule::consider_correct("", Language::English),
                RuleValidationError::EmptyWord,
            ),
            (
                NewUserRule::consider_correct(too_long, Language::English),
                RuleValidationError::WordTooLong { max: 128 },
            ),
            (
                NewUserRule::consider_correct("привет", Language::English),
                RuleValidationError::LanguageMismatch {
                    language: Language::English,
                    character: 'п',
                },
            ),
            (
                NewUserRule::consider_correct("helloмир", Language::English),
                RuleValidationError::LanguageMismatch {
                    language: Language::English,
                    character: 'м',
                },
            ),
            (
                NewUserRule::consider_correct("hello world", Language::English),
                RuleValidationError::InvalidWordCharacter { character: ' ' },
            ),
            (
                NewUserRule::consider_correct("hello2", Language::English),
                RuleValidationError::InvalidWordCharacter { character: '2' },
            ),
            (
                NewUserRule::consider_correct("-hello", Language::English),
                RuleValidationError::InvalidJoiner,
            ),
            (
                NewUserRule::consider_correct("hello--world", Language::English),
                RuleValidationError::InvalidJoiner,
            ),
            (
                NewUserRule::consider_correct("hello-", Language::English),
                RuleValidationError::InvalidJoiner,
            ),
        ];

        for (result, expected) in cases {
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn always_replace_validates_replacement_text() {
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", Language::English, ""),
            Err(RuleValidationError::EmptyReplacement)
        );
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", Language::English, " привет"),
            Err(RuleValidationError::ReplacementBoundaryWhitespace)
        );
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", Language::English, "привет\n"),
            Err(RuleValidationError::ReplacementBoundaryWhitespace)
        );
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", Language::English, "при\0вет"),
            Err(RuleValidationError::ReplacementControlCharacter)
        );
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", Language::English, "я".repeat(1025)),
            Err(RuleValidationError::ReplacementTooLong { max: 1024 })
        );
    }

    #[test]
    fn non_replacement_rules_never_carry_replacement_text() {
        let correct = NewUserRule::consider_correct("Rust", Language::English).unwrap();
        let never = NewUserRule::never_correct("cargo", Language::English).unwrap();

        assert_eq!(correct.replacement(), None);
        assert_eq!(never.replacement(), None);
    }
}
