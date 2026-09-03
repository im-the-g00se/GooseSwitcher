//! Configuration keys shared by the engine and the future settings application.

/// Enables automatic corrections.
pub const CORRECTIONS_ENABLED: &str = "corrections_enabled";
/// Minimum number of letters in an automatically corrected word.
pub const MINIMUM_WORD_LENGTH: &str = "minimum_word_length";
/// Shortcut for correcting the last word.
pub const CORRECT_LAST_WORD_SHORTCUT: &str = "correct_last_word_shortcut";
/// Shortcut for undoing the last correction.
pub const UNDO_LAST_CORRECTION_SHORTCUT: &str = "undo_last_correction_shortcut";
/// Shortcut for temporarily toggling corrections.
pub const TOGGLE_CORRECTIONS_SHORTCUT: &str = "toggle_corrections_shortcut";
