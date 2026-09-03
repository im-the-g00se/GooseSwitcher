//! Pure RU/EN layout conversion and word-correction decisions.

use std::collections::{BTreeSet, HashSet};

use crate::rules::{language_of_word, normalize_word, RuleAction, UserRule};

const ENGLISH_UNSHIFTED: &str = "`qwertyuiop[]asdfghjkl;'zxcvbnm,./";
const RUSSIAN_UNSHIFTED: &str = "ёйцукенгшщзхъфывапролджэячсмитьбю.";
const ENGLISH_SHIFTED: &str = "~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?";
const RUSSIAN_SHIFTED: &str = "ЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";

/// A word collection used by the recognition engine.
///
/// Lookups receive lowercase words. Implementations should therefore store
/// normalized lowercase entries or perform equivalent normalization.
pub trait Dictionary {
    /// Reports whether a lowercase word is present.
    fn contains(&self, normalized_word: &str) -> bool;
}

impl Dictionary for HashSet<String> {
    fn contains(&self, normalized_word: &str) -> bool {
        HashSet::contains(self, normalized_word)
    }
}

impl Dictionary for BTreeSet<String> {
    fn contains(&self, normalized_word: &str) -> bool {
        BTreeSet::contains(self, normalized_word)
    }
}

/// System dictionaries for the two layouts supported by the MVP.
#[derive(Clone, Copy)]
pub struct SystemDictionaries<'a> {
    russian: &'a dyn Dictionary,
    english: &'a dyn Dictionary,
}

impl<'a> SystemDictionaries<'a> {
    /// Creates a pair of Russian and English dictionaries.
    pub fn new(russian: &'a dyn Dictionary, english: &'a dyn Dictionary) -> Self {
        Self { russian, english }
    }

    fn contains(&self, normalized_word: &str) -> bool {
        self.russian.contains(normalized_word) || self.english.contains(normalized_word)
    }
}

/// Tunable recognition settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecognitionConfig {
    /// Whether automatic corrections are enabled.
    pub corrections_enabled: bool,
    /// Minimum number of letters in an automatically corrected token.
    pub minimum_word_length: usize,
}

impl Default for RecognitionConfig {
    fn default() -> Self {
        Self {
            corrections_enabled: true,
            minimum_word_length: 3,
        }
    }
}

/// Result of evaluating a completed word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorrectionDecision {
    /// Leave the source text unchanged without a matching user rule.
    Keep,
    /// Replace the source automatically using the dictionary-confidence rule.
    Replace(String),
    /// Apply a matching explicit user rule, including rules which keep text.
    ApplyUserRule {
        /// The requested user action.
        action: RuleAction,
        /// Replacement text for [`RuleAction::AlwaysReplace`].
        replacement: Option<String>,
    },
}

/// Converts characters produced by one physical RU/EN keyboard layout to the
/// characters produced by the other layout.
///
/// Cyrillic letters select RU-to-EN conversion. Otherwise EN-to-RU conversion
/// is used. Shifted keys are mapped to shifted keys, preserving letter case.
pub fn convert_layout(source: &str) -> String {
    let russian_to_english = source.chars().any(is_russian_letter);
    source
        .chars()
        .map(|character| {
            if russian_to_english {
                map_character(
                    character,
                    RUSSIAN_UNSHIFTED,
                    ENGLISH_UNSHIFTED,
                    RUSSIAN_SHIFTED,
                    ENGLISH_SHIFTED,
                )
            } else {
                map_character(
                    character,
                    ENGLISH_UNSHIFTED,
                    RUSSIAN_UNSHIFTED,
                    ENGLISH_SHIFTED,
                    RUSSIAN_SHIFTED,
                )
            }
        })
        .collect()
}

/// Reports whether an input character completes the current word.
///
/// Hyphen-minus and apostrophes remain inside a token so compound words and
/// contractions can be evaluated as a whole.
pub fn is_word_terminator(character: char) -> bool {
    if character.is_whitespace() {
        return true;
    }

    if matches!(character, '-' | '\'' | '’') {
        return false;
    }

    character.is_ascii_punctuation()
        || matches!(
            character,
            '…' | '—' | '–' | '«' | '»' | '„' | '“' | '”' | '‐' | '‑'
        )
}

/// Evaluates a source word using user rules, exclusions and system dictionaries.
///
/// User rules are matched exactly and take priority. Automatic replacement is
/// returned only when the converted word is known and the source word is not.
pub fn decide_correction<'rules, I>(
    source: &str,
    dictionaries: SystemDictionaries<'_>,
    user_rules: I,
    config: RecognitionConfig,
) -> CorrectionDecision
where
    I: IntoIterator<Item = &'rules UserRule>,
{
    if !config.corrections_enabled {
        return CorrectionDecision::Keep;
    }

    let source_language = language_of_word(source);
    let normalized_source = normalize_word(source);
    if let Some(rule) = user_rules.into_iter().find(|rule| {
        Some(rule.language) == source_language && rule.normalized_word == normalized_source
    }) {
        return CorrectionDecision::ApplyUserRule {
            action: rule.action,
            replacement: rule.replacement.clone(),
        };
    }

    if is_excluded(source, config.minimum_word_length) {
        return CorrectionDecision::Keep;
    }

    let converted = convert_layout(source);
    let normalized_source = normalize(source);
    let normalized_converted = normalize(&converted);

    if !dictionaries.contains(&normalized_source) && dictionaries.contains(&normalized_converted) {
        CorrectionDecision::Replace(converted)
    } else {
        CorrectionDecision::Keep
    }
}

fn map_character(
    character: char,
    source_unshifted: &str,
    target_unshifted: &str,
    source_shifted: &str,
    target_shifted: &str,
) -> char {
    source_unshifted
        .chars()
        .position(|candidate| candidate == character)
        .and_then(|index| target_unshifted.chars().nth(index))
        .or_else(|| {
            source_shifted
                .chars()
                .position(|candidate| candidate == character)
                .and_then(|index| target_shifted.chars().nth(index))
        })
        .unwrap_or(character)
}

fn normalize(word: &str) -> String {
    word.chars().flat_map(char::to_lowercase).collect()
}

fn is_excluded(source: &str, minimum_word_length: usize) -> bool {
    if source.is_empty() || looks_like_external_or_code_token(source) {
        return true;
    }

    let mut script = None;
    let mut letter_count = 0;
    let characters: Vec<char> = source.chars().collect();

    for (index, &character) in characters.iter().enumerate() {
        let character_script = if character.is_ascii_alphabetic() {
            Some(Script::English)
        } else if is_russian_letter(character) {
            Some(Script::Russian)
        } else {
            None
        };

        if let Some(character_script) = character_script {
            if script.is_some_and(|script| script != character_script) {
                return true;
            }
            script = Some(character_script);
            letter_count += 1;
            continue;
        }

        if is_joiner(character) {
            if index == 0
                || index + 1 == characters.len()
                || is_joiner(characters[index - 1])
                || is_joiner(characters[index + 1])
            {
                return true;
            }
            continue;
        }

        if !is_english_physical_letter_key(character) {
            return true;
        }
    }

    script.is_none() || letter_count < minimum_word_length
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    English,
    Russian,
}

fn is_russian_letter(character: char) -> bool {
    matches!(character, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

fn is_joiner(character: char) -> bool {
    matches!(character, '-' | '‐' | '‑' | '\'' | '’')
}

fn is_english_physical_letter_key(character: char) -> bool {
    "`~[]{};:'\",.<>".contains(character)
}

fn looks_like_external_or_code_token(source: &str) -> bool {
    let lowercase = source.to_ascii_lowercase();
    source.chars().any(|character| {
        character.is_numeric()
            || matches!(
                character,
                '/' | '\\'
                    | '_'
                    | '='
                    | '+'
                    | '*'
                    | '|'
                    | '&'
                    | '^'
                    | '%'
                    | '$'
                    | '#'
                    | '@'
                    | '!'
                    | '?'
                    | '('
                    | ')'
            )
    }) || lowercase.contains("://")
        || lowercase.starts_with("www.")
        || has_filename_or_domain_suffix(&lowercase)
        || source.contains("::")
        || source.contains("->")
        || source.contains("=>")
        || looks_like_camel_case_identifier(source)
}

fn has_filename_or_domain_suffix(lowercase: &str) -> bool {
    const SUFFIXES: [&str; 19] = [
        "com", "org", "net", "io", "ru", "рф", "txt", "md", "rs", "c", "h", "cpp", "py", "js",
        "ts", "json", "toml", "yaml", "yml",
    ];

    lowercase
        .rsplit_once('.')
        .is_some_and(|(stem, suffix)| !stem.is_empty() && SUFFIXES.contains(&suffix))
}

fn looks_like_camel_case_identifier(source: &str) -> bool {
    let mut saw_lowercase = false;
    for character in source.chars().filter(|character| character.is_alphabetic()) {
        if character.is_lowercase() {
            saw_lowercase = true;
        } else if saw_lowercase && character.is_uppercase() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::rules::{normalize_word, Language, RuleAction, UserRule};

    use super::*;

    fn dictionary(words: &[&str]) -> HashSet<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn rule(source: &str, action: RuleAction, replacement: Option<&str>) -> UserRule {
        UserRule {
            id: 1,
            word: source.to_owned(),
            normalized_word: normalize_word(source),
            language: if source.chars().any(super::is_russian_letter) {
                Language::Russian
            } else {
                Language::English
            },
            action,
            replacement: replacement.map(str::to_owned),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn decide_with(
        source: &str,
        russian: &[&str],
        english: &[&str],
        rules: &[UserRule],
        minimum_word_length: usize,
    ) -> CorrectionDecision {
        let russian = dictionary(russian);
        let english = dictionary(english);
        let dictionaries = SystemDictionaries::new(&russian, &english);

        decide_correction(
            source,
            dictionaries,
            rules,
            RecognitionConfig {
                minimum_word_length,
                ..RecognitionConfig::default()
            },
        )
    }

    #[test]
    fn converts_english_keys_to_russian_letters() {
        assert_eq!(convert_layout("ghbdtn"), "привет");
    }

    #[test]
    fn converts_russian_letters_to_english_keys() {
        assert_eq!(convert_layout("руддщ"), "hello");
    }

    #[test]
    fn conversion_preserves_physical_shift_state() {
        assert_eq!(convert_layout("GHBDTN"), "ПРИВЕТ");
        assert_eq!(convert_layout("ПРИВЕТ"), "GHBDTN");
        assert_eq!(convert_layout("GhBdTn"), "ПрИвЕт");
        assert_eq!(convert_layout("ЁЖЭБЮ"), "~:\"<>");
    }

    #[test]
    fn conversion_preserves_hyphens_and_typographic_apostrophes() {
        assert_eq!(convert_layout("руддщ-цщкдв"), "hello-world");
        assert_eq!(convert_layout("сфт’е"), "can’t");
    }

    #[test]
    fn whitespace_punctuation_and_enter_terminate_words() {
        for terminator in [
            ' ', '\t', '\n', '\r', ',', '.', '!', '?', ':', ';', '—', '…',
        ] {
            assert!(is_word_terminator(terminator), "{terminator:?}");
        }
    }

    #[test]
    fn letters_hyphens_and_apostrophes_do_not_terminate_words() {
        for character in ['a', 'я', '-', '\'', '’'] {
            assert!(!is_word_terminator(character), "{character:?}");
        }
    }

    #[test]
    fn replaces_only_unknown_source_with_known_conversion() {
        assert_eq!(
            decide_with("ghbdtn", &["привет"], &[], &[], 3),
            CorrectionDecision::Replace("привет".to_owned())
        );
        assert_eq!(
            decide_with("руддщ", &[], &["hello"], &[], 3),
            CorrectionDecision::Replace("hello".to_owned())
        );
        assert_eq!(
            decide_with("ghbdtn", &["привет"], &["ghbdtn"], &[], 3),
            CorrectionDecision::Keep
        );
        assert_eq!(
            decide_with("ghbdtn", &[], &[], &[], 3),
            CorrectionDecision::Keep
        );
    }

    #[test]
    fn dictionary_lookup_is_case_insensitive_but_replacement_keeps_case() {
        assert_eq!(
            decide_with("GHBDTN", &["привет"], &[], &[], 3),
            CorrectionDecision::Replace("ПРИВЕТ".to_owned())
        );
    }

    #[test]
    fn configurable_threshold_counts_letters_not_joiners() {
        assert_eq!(
            decide_with("руд", &[], &["hel"], &[], 4),
            CorrectionDecision::Keep
        );
        assert_eq!(
            decide_with("руд-д", &[], &["hel-l"], &[], 4),
            CorrectionDecision::Replace("hel-l".to_owned())
        );
    }

    #[test]
    fn valid_hyphenated_and_apostrophized_words_can_be_replaced() {
        assert_eq!(
            decide_with("руддщ-цщкдв", &[], &["hello-world"], &[], 3),
            CorrectionDecision::Replace("hello-world".to_owned())
        );
        assert_eq!(
            decide_with("сфтэе", &[], &["can't"], &[], 3),
            CorrectionDecision::Replace("can't".to_owned())
        );
    }

    #[test]
    fn malformed_joiners_are_excluded() {
        for (source, known_conversion) in [
            ("-ghbdtn", "-привет"),
            ("ghbdtn-", "привет-"),
            ("ghbdtn--vbh", "привет--мир"),
            ("'hello", "эруддщ"),
            ("hello''s", "руддщээы"),
        ] {
            assert_eq!(
                decide_with(source, &[known_conversion], &[], &[], 3),
                CorrectionDecision::Keep,
                "{source}"
            );
        }
    }

    #[test]
    fn urls_email_paths_numbers_and_code_like_tokens_are_excluded() {
        for source in [
            "https://ghbdtn.ru",
            "www.ghbdtn.ru",
            "ghbdtn@example.com",
            "/tmp/ghbdtn",
            "src\\ghbdtn",
            "12345",
            "ghbdtn2",
            "hello_world",
            "foo::bar",
            "value=ghbdtn",
        ] {
            assert_eq!(
                decide_with(source, &["привет"], &["hello"], &[], 1),
                CorrectionDecision::Keep,
                "{source}"
            );
        }
    }

    #[test]
    fn filename_like_tokens_are_excluded_even_if_the_conversion_is_known() {
        assert_eq!(
            decide_with("notes.txt", &["тщеуыюече"], &[], &[], 1),
            CorrectionDecision::Keep
        );
    }

    #[test]
    fn mixed_layout_and_non_word_tokens_are_excluded() {
        for source in ["ghbdтn", "hello🙂", ""] {
            assert_eq!(
                decide_with(source, &["привет"], &["hello"], &[], 1),
                CorrectionDecision::Keep,
                "{source}"
            );
        }
    }

    #[test]
    fn user_consider_correct_rule_has_priority_over_dictionaries() {
        let rules = [rule("ghbdtn", RuleAction::ConsiderCorrect, None)];

        assert_eq!(
            decide_with("GHBDTN", &["привет"], &[], &rules, 3),
            CorrectionDecision::ApplyUserRule {
                action: RuleAction::ConsiderCorrect,
                replacement: None,
            }
        );
    }

    #[test]
    fn user_never_correct_rule_has_priority_over_dictionaries() {
        let rules = [rule("ghbdtn", RuleAction::NeverCorrect, None)];

        assert_eq!(
            decide_with("ghbdtn", &["привет"], &[], &rules, 3),
            CorrectionDecision::ApplyUserRule {
                action: RuleAction::NeverCorrect,
                replacement: None,
            }
        );
    }

    #[test]
    fn user_replacement_has_priority_over_exclusions_and_dictionaries() {
        let rules = [rule(
            "ghbdtn",
            RuleAction::AlwaysReplace,
            Some("explicit replacement"),
        )];

        assert_eq!(
            decide_with("ghbdtn", &["привет"], &[], &rules, 100,),
            CorrectionDecision::ApplyUserRule {
                action: RuleAction::AlwaysReplace,
                replacement: Some("explicit replacement".to_owned()),
            }
        );
    }
}
