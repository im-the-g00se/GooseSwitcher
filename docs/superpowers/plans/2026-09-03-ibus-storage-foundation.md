# IBus and Storage Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Создать минимальный Rust-каркас GooseSwitcher и версионируемое SQLite-хранилище настроек и пользовательских правил.

**Architecture:** Один library crate публикует отдельные модули `ibus`, `config`, `storage`, `rules`, `recognition` и `settings_ui`. Исполняемая логика текущего этапа сосредоточена в конкретном `SqliteStore`; будущие IBus, recognition и GTK-компоненты остаются документированными границами без внешних зависимостей.

**Tech Stack:** Rust 2021, `rusqlite`, `directories`, SQLite 3, `tempfile` для тестов.

**Spec:** `docs/superpowers/specs/2026-09-03-ibus-storage-design.md`

## Global Constraints

- Поддерживаются только RU и EN; распознавание на этом этапе отсутствует.
- Не использовать глобальный перехват клавиатуры; будущая интеграция остаётся внутри IBus и совместима с Wayland.
- Не добавлять IBus, GTK4, libadwaita, systemd или RPM-реализацию на этом этапе.
- SQLite системная по умолчанию; feature `bundled-sqlite` разрешена только для локальной/CI-сборки без `sqlite-devel`.
- Пользовательские данные размещаются в `$XDG_DATA_HOME/gooseswitcher/gooseswitcher.db`, с fallback `$HOME/.local/share/gooseswitcher/gooseswitcher.db`.
- Каждое новое поведение разрабатывается циклом RED → GREEN → REFACTOR.

---

### Task 1: Cargo-каркас и границы модулей

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/ibus.rs`
- Create: `src/config.rs`
- Create: `src/recognition.rs`
- Create: `src/settings_ui.rs`
- Create: `src/rules.rs`
- Create: `src/storage/mod.rs`

**Interfaces:**
- Consumes: только границы из утверждённой спецификации.
- Produces: crate `gooseswitcher`; публичные модули `config`, `ibus`, `recognition`, `rules`, `settings_ui`, `storage`; opt-in feature `bundled-sqlite`.

- [ ] **Step 1: Создать манифест без production-логики**

```toml
[package]
name = "gooseswitcher"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "IBus layout correction engine for Russian and English"

[features]
default = []
bundled-sqlite = ["rusqlite/bundled"]

[dependencies]
directories = "6"
rusqlite = "0.37"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Объявить модули библиотеки**

```rust
//! Core library for GooseSwitcher.

pub mod config;
pub mod ibus;
pub mod recognition;
pub mod rules;
pub mod settings_ui;
pub mod storage;
```

В `ibus.rs`, `recognition.rs` и `settings_ui.rs` добавить только inner doc comments, явно описывающие будущую ответственность и отсутствие реализации на текущем этапе. В `config.rs` определить только строковые ключи `CORRECTIONS_ENABLED`, `CORRECT_LAST_WORD_SHORTCUT`, `UNDO_LAST_CORRECTION_SHORTCUT`, `TOGGLE_CORRECTIONS_SHORTCUT`. `rules.rs` и `storage/mod.rs` оставить с module docs до RED-тестов следующих задач.

- [ ] **Step 3: Проверить минимальный каркас**

Run: `cargo check --features bundled-sqlite`

Expected: exit 0; crate компилируется без IBus/GTK-зависимостей.

- [ ] **Step 4: Проверить форматирование**

Run: `cargo fmt --check`

Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "build: scaffold Rust package boundaries"
```

### Task 2: Инварианты пользовательских правил

**Files:**
- Modify: `src/rules.rs`

**Interfaces:**
- Consumes: имя модуля `rules` из Task 1.
- Produces: `RuleAction`, `NewUserRule`, `UserRule`, `RuleValidationError`; crate-private SQLite conversion methods `RuleAction::as_db_value` and `RuleAction::from_db_value`.

- [ ] **Step 1: Написать RED unit-тесты конструкторов**

Добавить в `src/rules.rs` тестовый модуль, который независимо задаёт ожидаемые значения:

```rust
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
```

Production mutation caught: удаление проверок пустого `source` или `replacement`, либо случайное сохранение replacement для правила без замены.

- [ ] **Step 2: Запустить тесты и подтвердить RED**

Run: `cargo test --features bundled-sqlite rules::tests -- --nocapture`

Expected: compilation failure because `NewUserRule` and `RuleValidationError` do not exist.

- [ ] **Step 3: Реализовать минимальную модель правил**

Определить:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleAction {
    ConsiderCorrect,
    NeverCorrect,
    AlwaysReplace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleValidationError {
    EmptySource,
    EmptyReplacement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewUserRule {
    source: String,
    action: RuleAction,
    replacement: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRule {
    pub id: i64,
    pub source: String,
    pub action: RuleAction,
    pub replacement: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Добавить три публичных конструктора, getters и SQLite-преобразование:

```rust
impl NewUserRule {
    fn validate_source(source: String) -> Result<String, RuleValidationError> {
        if source.is_empty() {
            Err(RuleValidationError::EmptySource)
        } else {
            Ok(source)
        }
    }

    pub fn consider_correct(source: impl Into<String>) -> Result<Self, RuleValidationError> {
        Ok(Self {
            source: Self::validate_source(source.into())?,
            action: RuleAction::ConsiderCorrect,
            replacement: None,
        })
    }

    pub fn never_correct(source: impl Into<String>) -> Result<Self, RuleValidationError> {
        Ok(Self {
            source: Self::validate_source(source.into())?,
            action: RuleAction::NeverCorrect,
            replacement: None,
        })
    }

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

    pub fn source(&self) -> &str { &self.source }
    pub fn action(&self) -> RuleAction { self.action }
    pub fn replacement(&self) -> Option<&str> { self.replacement.as_deref() }
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
```

Реализовать `Display` и `Error` для `RuleValidationError`, используя сообщения `rule source must not be empty` и `replacement must not be empty`.

- [ ] **Step 4: Подтвердить GREEN и выполнить refactor**

Run: `cargo test --features bundled-sqlite rules::tests`

Expected: 3 passed, 0 failed.

Run: `cargo fmt --check`

Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/rules.rs
git commit -m "feat: define user rule invariants"
```

### Task 3: XDG-путь, миграция и настройки

**Files:**
- Modify: `src/storage/mod.rs`
- Create: `src/storage/error.rs`
- Create: `src/storage/migrations.rs`
- Create: `src/storage/tests.rs`

**Interfaces:**
- Consumes: `directories::BaseDirs`, `rusqlite::Connection`, rule conversion API from Task 2.
- Produces: `SqliteStore::open_default`, `SqliteStore::open`, `Setting`, `StorageError`, settings CRUD; schema version 1.

- [ ] **Step 1: Написать RED unit-тест создания БД**

В `storage/mod.rs` подключить `#[cfg(test)] mod tests;`. В `storage/tests.rs` создать helper, возвращающий `(TempDir, PathBuf)`, и тесты настоящей файловой SQLite:

```rust
#[test]
fn opening_new_database_creates_version_one_schema() {
    let (_dir, path) = database_path();
    let store = SqliteStore::open(&path).unwrap();

    assert!(path.is_file());
    drop(store);
    let connection = rusqlite::Connection::open(path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert!(schema_has_table(&connection, "settings"));
    assert!(schema_has_table(&connection, "user_rules"));
}

```

Test helper `schema_has_table` выполняет параметризованный запрос к `sqlite_schema`; production тип не получает методов, нужных только тестам. Production mutations caught: отсутствие файла, одной из таблиц или `PRAGMA user_version = 1`.

- [ ] **Step 2: Запустить тест и подтвердить RED**

Run: `cargo test --features bundled-sqlite storage::tests::opening_new_database_creates_version_one_schema`

Expected: compilation failure because `SqliteStore` is missing.

- [ ] **Step 3: Реализовать открытие, migration 1 и ошибки**

В `migrations.rs` определить `LATEST_SCHEMA_VERSION: i64 = 1` и SQL из спецификации. В `SqliteStore::open` открыть `Connection`, включить `PRAGMA foreign_keys = ON`, прочитать `user_version`, отвергнуть версию выше 1 и применить migration 1 транзакцией для версии 0.

В `open_default` получить `BaseDirs::data_dir()`, добавить `gooseswitcher/gooseswitcher.db`, вызвать `create_dir_all`, затем `open`. Не читать и не изменять process environment в тестах.

`StorageError` должен иметь варианты:

```rust
pub enum StorageError {
    DataDirectoryUnavailable,
    CreateDataDirectory { path: PathBuf, source: io::Error },
    Database(rusqlite::Error),
    UnsupportedSchemaVersion { found: i64, latest: i64 },
    InvalidStoredAction(String),
    InvalidRule(RuleValidationError),
}
```

Реализовать `Display`, `Error::source` и `From<rusqlite::Error>`.

Основной migration flow:

```rust
fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: version,
            latest: LATEST_SCHEMA_VERSION,
        });
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    Ok(())
}
```

- [ ] **Step 4: Подтвердить GREEN создания**

Run: `cargo test --features bundled-sqlite storage::tests::opening_new_database_creates_version_one_schema`

Expected: 1 passed, 0 failed.

- [ ] **Step 5: Написать RED unit-тест полного CRUD настройки**

```rust
#[test]
fn setting_can_be_created_updated_and_deleted() {
    let (_dir, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();

    assert_eq!(store.get_setting("corrections_enabled").unwrap(), None);
    store.set_setting("corrections_enabled", "true").unwrap();
    assert_eq!(
        store.get_setting("corrections_enabled").unwrap().unwrap().value,
        "true"
    );
    store.set_setting("corrections_enabled", "false").unwrap();
    assert_eq!(
        store.get_setting("corrections_enabled").unwrap().unwrap().value,
        "false"
    );
    assert!(store.delete_setting("corrections_enabled").unwrap());
    assert!(!store.delete_setting("corrections_enabled").unwrap());
}

#[test]
fn reopening_database_preserves_setting() {
    let (_dir, path) = database_path();
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
```

Production mutations caught: INSERT без conflict-update, чтение неправильного ключа, потеря данных при повторном открытии, DELETE с неверной семантикой `bool`.

- [ ] **Step 6: Запустить тест и подтвердить RED**

Run: `cargo test --features bundled-sqlite storage::tests::setting_can_be_created_updated_and_deleted`

Expected: compilation failure because at least one settings CRUD method is missing.

- [ ] **Step 7: Реализовать минимальный CRUD настроек**

Добавить `Setting { pub key: String, pub value: String, pub updated_at: i64 }`. Использовать параметризованные SQL-запросы. `set_setting` выполняет `INSERT ... ON CONFLICT(key) DO UPDATE` и ставит Unix seconds UTC через `strftime('%s', 'now')`; `delete_setting` возвращает `changed_rows == 1`.

```rust
pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
    self.connection.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER))
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             updated_at = excluded.updated_at",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_setting(&self, key: &str) -> Result<Option<Setting>, StorageError> {
    self.connection
        .query_row(
            "SELECT key, value, updated_at FROM settings WHERE key = ?1",
            [key],
            |row| Ok(Setting { key: row.get(0)?, value: row.get(1)?, updated_at: row.get(2)? }),
        )
        .optional()
        .map_err(StorageError::from)
}

pub fn delete_setting(&mut self, key: &str) -> Result<bool, StorageError> {
    Ok(self.connection.execute("DELETE FROM settings WHERE key = ?1", [key])? == 1)
}
```

- [ ] **Step 8: Подтвердить GREEN задачи**

Run: `cargo test --features bundled-sqlite storage::tests`

Expected: все текущие storage-тесты проходят.

Run: `cargo fmt --check`

Expected: exit 0.

- [ ] **Step 9: Commit**

```bash
git add src/storage
git commit -m "feat: initialize SQLite settings storage"
```

### Task 4: CRUD пользовательских правил

**Files:**
- Modify: `src/storage/mod.rs`
- Modify: `src/storage/tests.rs`

**Interfaces:**
- Consumes: `NewUserRule`, `RuleAction`, `UserRule`, `StorageError`, таблица `user_rules`.
- Produces: `upsert_rule`, `get_rule`, `list_rules`, `delete_rule` с сигнатурами из спецификации.

- [ ] **Step 1: Написать RED unit-тест создания всех действий**

```rust
#[test]
fn every_rule_action_round_trips_through_sqlite() {
    let (_dir, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    let inputs = [
        NewUserRule::consider_correct("Rust").unwrap(),
        NewUserRule::never_correct("cargo").unwrap(),
        NewUserRule::always_replace("ghbdtn", "привет").unwrap(),
    ];

    for input in inputs {
        let source = input.source().to_owned();
        let saved = store.upsert_rule(input.clone()).unwrap();
        assert_eq!(store.get_rule(&source).unwrap(), Some(saved));
    }
}

#[test]
fn upsert_changes_action_without_replacing_identity() {
    let (_dir, path) = database_path();
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
```

Production mutations caught: неправильное SQL-представление любого действия, потеря replacement, чтение не той строки или upsert через замену identity.

- [ ] **Step 2: Запустить тест и подтвердить RED**

Run: `cargo test --features bundled-sqlite storage::tests::every_rule_action_round_trips_through_sqlite`

Expected: compilation failure because rule CRUD methods are missing.

- [ ] **Step 3: Реализовать upsert и get**

`upsert_rule(&mut self, rule)` выполняет транзакцию: параметризованный `INSERT ... ON CONFLICT(source) DO UPDATE SET action, replacement, updated_at`, затем SELECT по source. Conflict-update не изменяет `id` и `created_at`. Разбор неизвестного action возвращает `StorageError::InvalidStoredAction`.

```rust
pub fn upsert_rule(&mut self, rule: NewUserRule) -> Result<UserRule, StorageError> {
    let source = rule.source().to_owned();
    let transaction = self.connection.transaction()?;
    transaction.execute(
        "INSERT INTO user_rules
             (source, action, replacement, created_at, updated_at)
         VALUES
             (?1, ?2, ?3, CAST(strftime('%s', 'now') AS INTEGER),
              CAST(strftime('%s', 'now') AS INTEGER))
         ON CONFLICT(source) DO UPDATE SET
             action = excluded.action,
             replacement = excluded.replacement,
             updated_at = excluded.updated_at",
        params![rule.source(), rule.action().as_db_value(), rule.replacement()],
    )?;
    transaction.commit()?;
    self.get_rule(&source)?
        .ok_or_else(|| StorageError::Database(rusqlite::Error::QueryReturnedNoRows))
}
```

Чтобы неизвестное строковое действие оставалось отдельным `StorageError`, SQL row mapper сначала возвращает внутренний `StoredRule`, а conversion выполняется после `rusqlite` callback:

```rust
struct StoredRule {
    id: i64,
    source: String,
    action: String,
    replacement: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn stored_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRule> {
    Ok(StoredRule {
        id: row.get(0)?,
        source: row.get(1)?,
        action: row.get(2)?,
        replacement: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

impl TryFrom<StoredRule> for UserRule {
    type Error = StorageError;

    fn try_from(value: StoredRule) -> Result<Self, Self::Error> {
        let action = RuleAction::from_db_value(&value.action)
            .map_err(StorageError::InvalidStoredAction)?;
        Ok(Self {
            id: value.id,
            source: value.source,
            action,
            replacement: value.replacement,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

pub fn get_rule(&self, source: &str) -> Result<Option<UserRule>, StorageError> {
    let stored = self.connection.query_row(
        "SELECT id, source, action, replacement, created_at, updated_at
         FROM user_rules WHERE source = ?1",
        [source],
        stored_rule,
    ).optional()?;
    stored.map(UserRule::try_from).transpose()
}
```

- [ ] **Step 4: Подтвердить GREEN первого поведения**

Run: `cargo test --features bundled-sqlite storage::tests::every_rule_action_round_trips_through_sqlite`

Expected: оба теста upsert/get проходят.

- [ ] **Step 5: Написать RED unit-тест стабильного списка и удаления**

```rust
#[test]
fn rules_are_listed_by_source_and_delete_reports_presence() {
    let (_dir, path) = database_path();
    let mut store = SqliteStore::open(path).unwrap();
    store.upsert_rule(NewUserRule::consider_correct("zeta").unwrap()).unwrap();
    store.upsert_rule(NewUserRule::never_correct("alpha").unwrap()).unwrap();

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
```

Production mutations caught: отсутствие `ORDER BY source`, DELETE неправильного source, всегда истинный результат удаления.

- [ ] **Step 6: Запустить тест и подтвердить RED**

Run: `cargo test --features bundled-sqlite storage::tests::rules_are_listed_by_source_and_delete_reports_presence`

Expected: compilation failure because `list_rules` and `delete_rule` are missing.

- [ ] **Step 7: Реализовать list и delete, затем подтвердить GREEN**

Использовать общий приватный `stored_rule` из Step 3. `list_rules` выполняет `ORDER BY source ASC`; `delete_rule` возвращает `changed_rows == 1`.

```rust
pub fn list_rules(&self) -> Result<Vec<UserRule>, StorageError> {
    let mut statement = self.connection.prepare(
        "SELECT id, source, action, replacement, created_at, updated_at
         FROM user_rules ORDER BY source ASC",
    )?;
    let stored = statement.query_map([], stored_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    stored.into_iter().map(UserRule::try_from).collect()
}

pub fn delete_rule(&mut self, source: &str) -> Result<bool, StorageError> {
    Ok(self.connection.execute("DELETE FROM user_rules WHERE source = ?1", [source])? == 1)
}
```

Run: `cargo test --features bundled-sqlite`

Expected: все unit-тесты проходят.

Run: `cargo fmt --check`

Expected: exit 0.

- [ ] **Step 8: Commit**

```bash
git add src/rules.rs src/storage
git commit -m "feat: add user rule CRUD"
```

### Task 5: Документация формата и полная проверка

**Files:**
- Create: `README.md`
- Modify: `docs/mvp.md`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: фактические пути, schema version и команды из Tasks 1–4.
- Produces: краткая документация разработки и зафиксированного формата данных.

- [ ] **Step 1: Документировать только реализованное**

В `README.md` описать назначение текущего crate, системные зависимости Fedora (`cargo`, `rust`, `sqlite-devel`), команды `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` и opt-in `--features bundled-sqlite` для окружений без `sqlite-devel`.

В `docs/mvp.md` добавить раздел «Хранилище»: XDG data path, fallback, schema version 1 и названия таблиц. Не описывать установку IBus, UI, systemd или RPM как существующую.

- [ ] **Step 2: Проверить форматирование и diff**

Run: `cargo fmt --all --check`

Expected: exit 0.

Run: `git diff --check`

Expected: exit 0.

- [ ] **Step 3: Запустить полный test suite**

На Fedora с `sqlite-devel`:

Run: `cargo test --all-targets`

Expected: все тесты проходят.

В текущей среде без `sqlite-devel`:

Run: `cargo test --all-targets --features bundled-sqlite`

Expected: все тесты проходят.

- [ ] **Step 4: Запустить линтер с максимальным набором target/features**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: exit 0, warnings отсутствуют.

- [ ] **Step 5: Сверить требования и scope**

Run: `git status --short && git diff --stat && git diff`

Проверить буквально: присутствуют шесть требуемых модулей; база имеет правильный XDG data path; settings и три действия user rules покрыты схемой/API/тестами; отсутствуют IBus/GTK/systemd/RPM и recognition implementations; нет изменений несвязанного кода.

- [ ] **Step 6: Commit**

```bash
git add README.md docs/mvp.md Cargo.lock
git commit -m "docs: describe storage development setup"
```
