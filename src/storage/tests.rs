use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};
use tempfile::TempDir;

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
