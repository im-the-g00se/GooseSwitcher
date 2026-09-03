//! User dictionary rule types.

use std::{error::Error, fmt};

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

/// A validated rule ready to be persisted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewUserRule {
    source: String,
    action: RuleAction,
    replacement: Option<String>,
}

impl NewUserRule {
    /// Creates a rule that treats `source` as correct.
    pub fn consider_correct(source: impl Into<String>) -> Result<Self, RuleValidationError> {
        Ok(Self {
            source: Self::validate_source(source.into())?,
            action: RuleAction::ConsiderCorrect,
            replacement: None,
        })
    }

    /// Creates a rule that prevents automatic correction of `source`.
    pub fn never_correct(source: impl Into<String>) -> Result<Self, RuleValidationError> {
        Ok(Self {
            source: Self::validate_source(source.into())?,
            action: RuleAction::NeverCorrect,
            replacement: None,
        })
    }

    /// Creates a rule that always replaces `source` with `replacement`.
    pub fn always_replace(
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Result<Self, RuleValidationError> {
        let replacement = replacement.into();
        if replacement.is_empty() {
            return Err(RuleValidationError::EmptyReplacement);
        }

        Ok(Self {
            source: Self::validate_source(source.into())?,
            action: RuleAction::AlwaysReplace,
            replacement: Some(replacement),
        })
    }

    /// Returns the token matched by this rule.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the action performed by this rule.
    pub fn action(&self) -> RuleAction {
        self.action
    }

    /// Returns replacement text for an always-replace rule.
    pub fn replacement(&self) -> Option<&str> {
        self.replacement.as_deref()
    }

    fn validate_source(source: String) -> Result<String, RuleValidationError> {
        if source.is_empty() {
            Err(RuleValidationError::EmptySource)
        } else {
            Ok(source)
        }
    }
}

/// A user dictionary rule loaded from storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRule {
    /// Stable database identifier.
    pub id: i64,
    /// Token matched by the rule.
    pub source: String,
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
    /// The source token is empty.
    EmptySource,
    /// An always-replace rule has an empty replacement.
    EmptyReplacement,
}

impl fmt::Display for RuleValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => formatter.write_str("rule source must not be empty"),
            Self::EmptyReplacement => formatter.write_str("replacement must not be empty"),
        }
    }
}

impl Error for RuleValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_replace_requires_non_empty_replacement() {
        assert_eq!(
            NewUserRule::always_replace("ghbdtn", ""),
            Err(RuleValidationError::EmptyReplacement)
        );
    }

    #[test]
    fn non_replacement_rules_never_carry_replacement_text() {
        let correct = NewUserRule::consider_correct("Rust").unwrap();
        let never = NewUserRule::never_correct("cargo").unwrap();

        assert_eq!(correct.replacement(), None);
        assert_eq!(never.replacement(), None);
    }

    #[test]
    fn empty_source_is_rejected() {
        assert_eq!(
            NewUserRule::consider_correct(""),
            Err(RuleValidationError::EmptySource)
        );
    }
}
