use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};
use tempfile::TempDir;

use crate::rules::{NewUserRule, RuleAction};

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

#[test]
fn opening_new_database_creates_version_one_schema() {
    let (_directory, path) = database_path();
    let store = SqliteStore::open(&path).unwrap();

    assert!(path.is_file());
    drop(store);

    let connection = Connection::open(path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert!(schema_has_table(&connection, "settings"));
    assert!(schema_has_table(&connection, "user_rules"));
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
fn every_rule_action_round_trips_through_sqlite() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    let inputs = [
        NewUserRule::consider_correct("Rust").unwrap(),
        NewUserRule::never_correct("cargo").unwrap(),
        NewUserRule::always_replace("ghbdtn", "привет").unwrap(),
    ];

    for input in inputs {
        let source = input.source().to_owned();
        let saved = store.upsert_rule(input).unwrap();
        assert_eq!(store.get_rule(&source).unwrap(), Some(saved));
    }
}

#[test]
fn upsert_changes_action_without_replacing_identity() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    let original = store
        .upsert_rule(NewUserRule::never_correct("руддщ").unwrap())
        .unwrap();
    let updated = store
        .upsert_rule(NewUserRule::always_replace("руддщ", "hello").unwrap())
        .unwrap();

    assert_eq!(updated.id, original.id);
    assert_eq!(updated.created_at, original.created_at);
    assert_eq!(updated.action, RuleAction::AlwaysReplace);
    assert_eq!(updated.replacement.as_deref(), Some("hello"));
}

#[test]
fn rules_are_listed_by_source_and_delete_reports_presence() {
    let (_directory, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    store
        .upsert_rule(NewUserRule::consider_correct("zeta").unwrap())
        .unwrap();
    store
        .upsert_rule(NewUserRule::never_correct("alpha").unwrap())
        .unwrap();

    let sources: Vec<_> = store
        .list_rules()
        .unwrap()
        .into_iter()
        .map(|rule| rule.source)
        .collect();
    assert_eq!(sources, ["alpha", "zeta"]);
    assert!(store.delete_rule("alpha").unwrap());
    assert!(!store.delete_rule("alpha").unwrap());
    assert_eq!(store.get_rule("alpha").unwrap(), None);
}
