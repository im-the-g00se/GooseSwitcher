use std::{fs, path::Path};

use gooseswitcher::{
    config::{CORRECTIONS_ENABLED, MINIMUM_WORD_LENGTH},
    dictionaries::{DictionaryLoadError, SystemDictionaryIndex, SystemDictionaryPaths},
    engine::RecognitionRuntime,
    recognition::{CorrectionDecision, RecognitionConfig},
    rules::{Language, NewUserRule},
    storage::SqliteStore,
};
use tempfile::TempDir;

fn write_dictionary(directory: &Path, locale: &str, aff: &[u8], dic: &[u8]) {
    fs::write(directory.join(format!("{locale}.aff")), aff).unwrap();
    fs::write(directory.join(format!("{locale}.dic")), dic).unwrap();
}

fn fixture_paths() -> (TempDir, SystemDictionaryPaths) {
    let directory = tempfile::tempdir().unwrap();
    write_dictionary(
        directory.path(),
        "en_US",
        b"SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n",
        b"1\nhello/S\n",
    );
    write_dictionary(
        directory.path(),
        "ru_RU",
        "SET UTF-8\n".as_bytes(),
        "1\nпривет\n".as_bytes(),
    );
    let paths = SystemDictionaryPaths::from_directory(directory.path());
    (directory, paths)
}

fn store() -> (TempDir, SqliteStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(directory.path().join("settings.db")).unwrap();
    (directory, store)
}

#[test]
fn loaded_hunspell_dictionaries_recognize_stems_and_affixed_words() {
    let (_dictionary_directory, paths) = fixture_paths();
    let dictionaries = SystemDictionaryIndex::load(&paths).unwrap();

    assert!(dictionaries.russian_contains("привет"));
    assert!(dictionaries.english_contains("hello"));
    assert!(dictionaries.english_contains("hellos"));
    assert!(!dictionaries.english_contains("привет"));
}

#[test]
fn koi8_r_dictionary_is_decoded_before_indexing() {
    let directory = tempfile::tempdir().unwrap();
    write_dictionary(directory.path(), "en_US", b"SET UTF-8\n", b"0\n");
    write_dictionary(
        directory.path(),
        "ru_RU",
        b"SET KOI8-R\n",
        b"1\n\xd0\xd2\xc9\xd7\xc5\xd4\n",
    );
    let paths = SystemDictionaryPaths::from_directory(directory.path());

    let dictionaries = SystemDictionaryIndex::load(&paths).unwrap();

    assert!(dictionaries.russian_contains("привет"));
}

#[test]
fn empty_dictionary_files_create_an_index_that_matches_nothing() {
    let directory = tempfile::tempdir().unwrap();
    write_dictionary(directory.path(), "en_US", b"", b"");
    write_dictionary(directory.path(), "ru_RU", b"", b"");
    let paths = SystemDictionaryPaths::from_directory(directory.path());

    let dictionaries = SystemDictionaryIndex::load(&paths).unwrap();

    assert!(!dictionaries.russian_contains("привет"));
    assert!(!dictionaries.english_contains("hello"));
}

#[test]
fn unavailable_dictionary_reports_the_exact_missing_path() {
    let directory = tempfile::tempdir().unwrap();
    let paths = SystemDictionaryPaths::from_directory(directory.path());

    let error = SystemDictionaryIndex::load(&paths).unwrap_err();

    match error {
        DictionaryLoadError::Read { path, .. } => {
            assert_eq!(path, directory.path().join("ru_RU.aff"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn runtime_uses_settings_and_user_rules_snapshotted_at_startup() {
    let (_dictionary_directory, paths) = fixture_paths();
    let (_database_directory, mut store) = store();
    store.set_setting(CORRECTIONS_ENABLED, "true").unwrap();
    store.set_setting(MINIMUM_WORD_LENGTH, "4").unwrap();
    let saved = store
        .create_rule(
            NewUserRule::always_replace("ghbdtn", Language::English, "добрый день").unwrap(),
        )
        .unwrap();

    let runtime = RecognitionRuntime::load(&store, &paths).unwrap();
    store.delete_rule(saved.id).unwrap();
    store.set_setting(CORRECTIONS_ENABLED, "false").unwrap();
    drop(store);

    assert_eq!(
        runtime.decide("GHBDTN"),
        CorrectionDecision::ApplyUserRule {
            action: gooseswitcher::rules::RuleAction::AlwaysReplace,
            replacement: Some("добрый день".to_owned()),
        }
    );
    assert_eq!(runtime.decide("руд"), CorrectionDecision::Keep);
}

#[test]
fn disabled_corrections_keep_words_even_when_a_rule_or_dictionary_matches() {
    let (_dictionary_directory, paths) = fixture_paths();
    let (_database_directory, mut store) = store();
    store.set_setting(CORRECTIONS_ENABLED, "false").unwrap();
    store
        .create_rule(
            NewUserRule::always_replace("ghbdtn", Language::English, "добрый день").unwrap(),
        )
        .unwrap();
    let runtime = RecognitionRuntime::load(&store, &paths).unwrap();

    assert_eq!(runtime.decide("ghbdtn"), CorrectionDecision::Keep);
    assert!(!runtime.config().corrections_enabled);
}

#[test]
fn absent_settings_use_documented_defaults() {
    let (_dictionary_directory, paths) = fixture_paths();
    let (_database_directory, store) = store();

    let runtime = RecognitionRuntime::load(&store, &paths).unwrap();

    assert_eq!(runtime.config(), RecognitionConfig::default());
    assert_eq!(
        runtime.decide("руддщы"),
        CorrectionDecision::Replace("hellos".to_owned())
    );
}
