use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use tempfile::TempDir;

use crate::rules::{Language, NewUserRule, RuleAction};

use super::*;

fn database_path() -> (TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gooseswitcher.db");
    (directory, path)
}

fn schema_has_table(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()
        .unwrap()
        .is_some()
}

fn schema_has_column(connection: &Connection, table: &str, column: &str) -> bool {
    let query = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1");
    connection
        .query_row(&query, [column], |_| Ok(()))
        .optional()
        .unwrap()
        .is_some()
}

fn create_version_one_database(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(MIGRATION_1).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
}

#[test]
fn opening_new_database_creates_version_two_schema() {
    let (_directory, path) = database_path();
    let store = SqliteStore::open(&path).unwrap();

    assert!(path.is_file());
    drop(store);

    let connection = Connection::open(path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    assert!(schema_has_table(&connection, "settings"));
    assert!(schema_has_table(&connection, "user_rules"));
    assert!(schema_has_column(
        &connection,
        "user_rules",
        "normalized_word"
    ));
    assert!(schema_has_column(&connection, "user_rules", "language"));
}

#[test]
fn version_one_rules_are_migrated_without_losing_identity_or_timestamps() {
    let (_directory, path) = database_path();
    let connection = create_version_one_database(&path);
    connection
        .execute(
            "INSERT INTO user_rules
             (id, source, action, replacement, created_at, updated_at)
             VALUES (7, 'Rust', 'consider_correct', NULL, 100, 200),
                    (9, 'привет', 'always_replace', 'здравствуйте', 300, 400)",
            [],
        )
        .unwrap();
    drop(connection);

    let store = SqliteStore::open(&path).unwrap();
    let rust = store.find_rule(Language::English, "RUST").unwrap().unwrap();
    let hello = store
        .find_rule(Language::Russian, "Привет")
        .unwrap()
        .unwrap();

    assert_eq!((rust.id, rust.created_at, rust.updated_at), (7, 100, 200));
    assert_eq!(rust.word, "Rust");
    assert_eq!(rust.normalized_word, "rust");
    assert_eq!(rust.action, RuleAction::ConsiderCorrect);
    assert_eq!(
        (hello.id, hello.created_at, hello.updated_at),
        (9, 300, 400)
    );
    assert_eq!(hello.language, Language::Russian);
    assert_eq!(hello.replacement.as_deref(), Some("здравствуйте"));
}

#[test]
fn conflicting_version_one_rules_roll_back_migration() {
    let (_directory, path) = database_path();
    let connection = create_version_one_database(&path);
    connection
        .execute(
            "INSERT INTO user_rules
             (source, action, replacement, created_at, updated_at)
             VALUES ('Rust', 'consider_correct', NULL, 100, 100),
                    ('rust', 'never_correct', NULL, 200, 200)",
            [],
        )
        .unwrap();
    drop(connection);

    let error = SqliteStore::open(&path).unwrap_err();
    assert!(matches!(error, StorageError::MigrationConflict { .. }));

    let connection = Connection::open(path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert!(schema_has_column(&connection, "user_rules", "source"));
    assert!(!schema_has_table(&connection, "user_rules_v2"));
}

#[test]
fn invalid_version_one_rule_rolls_back_migration() {
    let (_directory, path) = database_path();
    let connection = create_version_one_database(&path);
    connection
        .execute(
            "INSERT INTO user_rules
             (id, source, action, replacement, created_at, updated_at)
             VALUES (4, 'bad value', 'consider_correct', NULL, 100, 100)",
            [],
        )
        .unwrap();
    drop(connection);

    let error = SqliteStore::open(&path).unwrap_err();
    match error {
        StorageError::MigrationRule { id, word, .. } => {
            assert_eq!(id, 4);
            assert_eq!(word, "bad value");
        }
        other => panic!("unexpected error: {other}"),
    }

    let connection = Connection::open(path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert!(!schema_has_table(&connection, "user_rules_v2"));
}

#[test]
fn setting_can_be_created_updated_and_deleted() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();

    assert_eq!(store.get_setting("corrections_enabled").unwrap(), None);
    store.set_setting("corrections_enabled", "true").unwrap();
    assert_eq!(
        store
            .get_setting("corrections_enabled")
            .unwrap()
            .unwrap()
            .value,
        "true"
    );
    store.set_setting("corrections_enabled", "false").unwrap();
    assert_eq!(
        store
            .get_setting("corrections_enabled")
            .unwrap()
            .unwrap()
            .value,
        "false"
    );
    assert!(store.delete_setting("corrections_enabled").unwrap());
    assert!(!store.delete_setting("corrections_enabled").unwrap());
}

#[test]
fn reopening_database_preserves_setting() {
    let (_directory, path) = database_path();
    let mut initial = SqliteStore::open(&path).unwrap();
    initial.set_setting("corrections_enabled", "true").unwrap();
    drop(initial);

    let reopened = SqliteStore::open(&path).unwrap();
    let setting = reopened
        .get_setting("corrections_enabled")
        .unwrap()
        .unwrap();
    assert_eq!(setting.key, "corrections_enabled");
    assert_eq!(setting.value, "true");
    assert!(setting.updated_at > 0);
}

#[test]
fn rule_can_be_created_read_updated_and_deleted_by_id() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    let created = store
        .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
        .unwrap();

    assert_eq!(store.get_rule(created.id).unwrap(), Some(created.clone()));
    assert_eq!(
        store.find_rule(Language::English, "RUST").unwrap(),
        Some(created.clone())
    );

    let updated = store
        .update_rule(
            created.id,
            NewUserRule::always_replace("Cargo", Language::English, "груз").unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.word, "Cargo");
    assert_eq!(updated.action, RuleAction::AlwaysReplace);
    assert_eq!(updated.replacement.as_deref(), Some("груз"));
    assert_eq!(
        store
            .update_rule(
                i64::MAX,
                NewUserRule::never_correct("missing", Language::English).unwrap(),
            )
            .unwrap(),
        None
    );
    assert!(store.delete_rule(created.id).unwrap());
    assert!(!store.delete_rule(created.id).unwrap());
    assert_eq!(store.get_rule(created.id).unwrap(), None);
}

#[test]
fn normalized_word_and_language_are_the_unique_rule_key() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    store
        .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
        .unwrap();

    let error = store
        .create_rule(NewUserRule::never_correct("rust", Language::English).unwrap())
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::DuplicateRule {
            language: Language::English,
            ref normalized_word
        } if normalized_word == "rust"
    ));
}

#[test]
fn search_filters_normalizes_sorts_and_paginates() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    let rules = [
        NewUserRule::never_correct("Alpine", Language::English).unwrap(),
        NewUserRule::consider_correct("alpha", Language::English).unwrap(),
        NewUserRule::never_correct("beta", Language::English).unwrap(),
        NewUserRule::never_correct("альфа", Language::Russian).unwrap(),
    ];
    for rule in rules {
        store.create_rule(rule).unwrap();
    }

    let found = store
        .search_rules(&RuleSearch {
            text: Some("ALP".to_owned()),
            language: Some(Language::English),
            action: None,
            limit: 100,
            offset: 0,
        })
        .unwrap();
    assert_eq!(
        found.into_iter().map(|rule| rule.word).collect::<Vec<_>>(),
        ["alpha", "Alpine"]
    );

    let paged = store
        .search_rules(&RuleSearch {
            text: None,
            language: Some(Language::English),
            action: Some(RuleAction::NeverCorrect),
            limit: 1,
            offset: 1,
        })
        .unwrap();
    assert_eq!(paged[0].word, "beta");
}

#[test]
fn search_rejects_limits_outside_documented_range() {
    let (_directory, path) = database_path();
    let store = SqliteStore::open(path).unwrap();

    for limit in [0, 501] {
        let error = store
            .search_rules(&RuleSearch {
                limit,
                ..RuleSearch::default()
            })
            .unwrap_err();
        assert!(matches!(
            error,
            StorageError::InvalidSearchLimit { found } if found == limit
        ));
    }
}

#[test]
fn search_rejects_an_offset_that_sqlite_cannot_represent() {
    let (_directory, path) = database_path();
    let store = SqliteStore::open(path).unwrap();

    let error = store
        .search_rules(&RuleSearch {
            offset: usize::MAX,
            ..RuleSearch::default()
        })
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::InvalidSearchOffset { found } if found == usize::MAX
    ));
}

#[test]
fn database_rejects_invalid_language_and_action_values() {
    let (_directory, path) = database_path();
    let store = SqliteStore::open(&path).unwrap();
    drop(store);
    let connection = Connection::open(path).unwrap();

    let invalid_language = connection.execute(
        "INSERT INTO user_rules
         (word, normalized_word, language, action, replacement, created_at, updated_at)
         VALUES ('hello', 'hello', 'de', 'consider_correct', NULL, 1, 1)",
        [],
    );
    assert!(invalid_language.is_err());

    let invalid_action = connection.execute(
        "INSERT INTO user_rules
         (word, normalized_word, language, action, replacement, created_at, updated_at)
         VALUES (?1, ?2, 'en', 'unknown', NULL, 1, 1)",
        params!["hello", "hello"],
    );
    assert!(invalid_action.is_err());
}

#[test]
fn reading_a_corrupted_normalized_key_returns_a_specific_error() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(&path).unwrap();
    let rule = store
        .create_rule(NewUserRule::consider_correct("Rust", Language::English).unwrap())
        .unwrap();
    drop(store);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE user_rules SET normalized_word = 'wrong' WHERE id = ?1",
            [rule.id],
        )
        .unwrap();
    drop(connection);

    let store = SqliteStore::open(path).unwrap();
    let error = store.get_rule(rule.id).unwrap_err();
    assert!(matches!(
        error,
        StorageError::InvalidStoredNormalizedWord {
            ref word,
            ref expected,
            ref found
        } if word == "Rust" && expected == "rust" && found == "wrong"
    ));
}
