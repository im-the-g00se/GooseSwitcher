# User Dictionary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Реализовать валидируемый RU/EN пользовательский словарь с полноценным CRUD, поиском, миграцией SQLite v1→v2, атомарным JSON-обменом и приоритетом правил в runtime.

**Architecture:** Модель и инварианты живут в `rules`, SQLite CRUD и миграции — в `storage`, переносимый JSON — в отдельном `user_dictionary`, а `RecognitionRuntime` использует нормализованный ключ `(Language, word)` из снимка правил. Публичные операции не выполняют скрытый upsert; импорт валидируется целиком и изменяет базу одной транзакцией.

**Tech Stack:** Rust 2021, rusqlite 0.37, serde 1 с derive, serde_json 1, tempfile.

**Spec:** `docs/superpowers/specs/2026-09-03-user-dictionary-design.md`

## Global Constraints

- Поддерживаются только RU и EN.
- Не добавлять GTK, IBus, глобальный перехват клавиатуры, контекстную или частотную модель.
- Не менять словарную автозамену за пределами разрешения пользовательских правил до системных словарей.
- Новая логика разрабатывается циклами RED → GREEN → REFACTOR на настоящей временной SQLite без mock.
- Импорт и миграция атомарны; дубликаты и повреждённые данные не перезаписываются молча.
- JSON UTF-8 имеет имя `gooseswitcher-user-dictionary`, версию 1 и документируется.
- Новая схема SQLite имеет `PRAGMA user_version = 2`.

---

### Task 1: Язык, нормализация и валидация модели

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/rules.rs`

**Interfaces:**
- Consumes: существующий `RuleAction`.
- Produces: `Language`, `NewUserRule::{consider_correct, never_correct, always_replace}`, getters `word`, `normalized_word`, `language`, `action`, `replacement`; crate-visible `normalize_word`; расширенный `RuleValidationError`.

- [ ] **Step 1: Написать RED-тесты допустимых языков и нормализации**

Добавить литеральные проверки:

```rust
#[test]
fn valid_words_are_normalized_without_changing_display_spelling() {
    let english = NewUserRule::consider_correct("Rust", Language::English).unwrap();
    let russian = NewUserRule::never_correct("Ёлка-парк", Language::Russian).unwrap();
    assert_eq!(english.word(), "Rust");
    assert_eq!(english.normalized_word(), "rust");
    assert_eq!(russian.normalized_word(), "ёлка-парк");
}
```

Production mutation caught: сохранение исходного регистра в ключе или смешивание языка с текстом.

- [ ] **Step 2: Написать RED-таблицу ошибок слова и замены**

Проверить пустое/длиннее 128, несовместимую письменность, смешанные буквы, крайние/двойные соединители, пробелы и цифры. Для замены проверить пустое/длиннее 1024, крайние пробелы и управляющие символы. Каждая строка сравнивает конкретный `RuleValidationError`.

- [ ] **Step 3: Запустить RED**

Run: `cargo test --features bundled-sqlite rules::tests`

Expected: compilation failure because `Language` and the language parameters do not exist.

- [ ] **Step 4: Реализовать минимальную модель**

Добавить:

```rust
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Language { Russian, English }

pub(crate) fn normalize_word(word: &str) -> String {
    word.chars().flat_map(char::to_lowercase).collect()
}
```

`Language::{as_db_value, from_db_value}` использует `ru`/`en`. Конструкторы принимают `(word, language)` и вызывают общий `validate_word`; `always_replace` принимает `(word, language, replacement)`. Ошибки несут точную причину: `EmptyWord`, `WordTooLong`, `InvalidWordCharacter`, `LanguageMismatch`, `InvalidJoiner`, `EmptyReplacement`, `ReplacementTooLong`, `ReplacementBoundaryWhitespace`, `ReplacementControlCharacter`.

- [ ] **Step 5: Подтвердить GREEN**

Run: `cargo test --features bundled-sqlite rules::tests`

Expected: all rule tests pass.

- [ ] **Step 6: Добавить JSON-зависимости и проверить сборку**

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Run: `cargo check --all-targets --features bundled-sqlite`

Expected: exit 0.

### Task 2: Миграция v2 и полноценный SQLite CRUD/search

**Files:**
- Modify: `src/storage/migrations.rs`
- Modify: `src/storage/mod.rs`
- Modify: `src/storage/error.rs`
- Modify: `src/storage/tests.rs`
- Modify: `tests/dictionaries_and_settings.rs`

**Interfaces:**
- Consumes: `Language`, `NewUserRule`, `UserRule`, `RuleAction`.
- Produces: `RuleSearch`, `SqliteStore::{create_rule, update_rule, get_rule, find_rule, search_rules, delete_rule}`; crate-visible transaction helpers for import; schema version 2.

- [ ] **Step 1: Написать RED-тест новой базы и миграции v1**

Создать v1 fixture прежним SQL, вставить `Rust` и `привет`, открыть через `SqliteStore`, затем проверить `user_version = 2`, сохранённые ID/даты/действия и выведенные языки. Отдельный тест вставляет `Rust` и `rust` в v1 и проверяет ошибку миграционного конфликта и оставшийся `user_version = 1`.

- [ ] **Step 2: Запустить migration RED**

Run: `cargo test --features bundled-sqlite storage::tests`

Expected: failure because the current version is 1 and language columns are absent.

- [ ] **Step 3: Реализовать последовательные миграции**

`MIGRATION_1` остаётся историческим SQL. Добавить `MIGRATION_2_CREATE`, `MIGRATION_2_FINISH`, `LATEST_SCHEMA_VERSION = 2`. Для версии 0 применить v1, поставить `user_version = 1`, затем выполнить Rust migration 1→2. Каждая версия фиксируется только внутри своей транзакции; ошибка валидации или unique constraint отображается в `MigrationRule`/`MigrationConflict` и откатывает v2.

- [ ] **Step 4: Подтвердить migration GREEN**

Run: `cargo test --features bundled-sqlite storage::tests`

Expected: all selected tests pass.

- [ ] **Step 5: Написать RED-тест CRUD и дубликата**

Тест создаёт правило, получает его по ID и `(language, word)`, обновляет слово/тип с сохранением ID/`created_at`, удаляет по ID и получает `false` при повторном удалении. Второй тест создаёт `Rust`, затем `rust` и ожидает `StorageError::DuplicateRule { language: English, normalized_word: "rust" }`.

- [ ] **Step 6: Написать RED-тест поиска**

Создать RU/EN правила разных типов. Проверить lowercase substring, фильтры языка/типа, стабильный порядок, `limit`/`offset`, а также ошибки нулевого и превышающего 500 лимита.

- [ ] **Step 7: Запустить CRUD/search RED**

Run: `cargo test --features bundled-sqlite storage::tests`

Expected: compilation failure because the explicit CRUD and `RuleSearch` do not exist.

- [ ] **Step 8: Реализовать CRUD/search минимально**

Все запросы выбирают `id, word, normalized_word, language, action, replacement, created_at, updated_at`. `create_rule` использует INSERT без conflict-update; `update_rule` обновляет по ID; unique constraint маппится через предварительный lookup ключа и проверку SQLite extended code в `DuplicateRule`. `search_rules` строит SQL из конечного набора безопасных ветвей с параметрами, использует `instr(normalized_word, ?)` и `ORDER BY normalized_word, language, id LIMIT ? OFFSET ?`.

- [ ] **Step 9: Перевести существующих потребителей с upsert на create**

В `tests/dictionaries_and_settings.rs` передать явный язык и заменить `upsert_rule` на `create_rule`. Удаление выполнять по возвращённому ID. Удалить старые `upsert_rule`, `get_rule(source)`, `list_rules`, `delete_rule(source)` после перевода engine в Task 4; до этого оставить crate-private адаптер загрузки всех записей.

- [ ] **Step 10: Подтвердить GREEN и refactor**

Run: `cargo test --all-targets --features bundled-sqlite`

Expected: all tests pass.

### Task 3: Атомарный импорт и детерминированный экспорт JSON

**Files:**
- Create: `src/user_dictionary.rs`
- Modify: `src/lib.rs`
- Modify: `src/storage/mod.rs`
- Modify: `src/storage/error.rs`

**Interfaces:**
- Consumes: storage CRUD/transaction primitives and validated rules.
- Produces: `ImportConflictPolicy::{Reject, Skip, Replace}`, `ImportReport`, `DictionaryTransferError`, `export_json`, `import_json`.

- [ ] **Step 1: Написать RED round-trip тест**

Создать по одному правилу каждого типа, экспортировать в `Vec<u8>`, проверить точный pretty JSON с `format`, `version`, стабильным порядком и отсутствующим `replacement` у двух правил, импортировать в пустую базу и сравнить переносимые поля.

- [ ] **Step 2: Написать RED-тесты строгой валидации документа**

Отдельными литеральными JSON fixtures проверить неверные `format`/`version`, неизвестное поле, неизвестные язык/тип, replacement у несовместимого типа, документ больше 10 MiB, больше 10 000 записей и внутренние дубликаты `Rust`/`rust`.

- [ ] **Step 3: Написать RED-тесты политик конфликта и атомарности**

Для существующего `Rust` проверить: `Reject` возвращает конфликт и не создаёт соседнюю новую запись; `Skip` возвращает `{ created: 1, updated: 0, skipped: 1 }`; `Replace` сохраняет ID/`created_at` и возвращает `{ created: 1, updated: 1, skipped: 0 }`. Повреждённая вторая запись не должна сохранить первую.

- [ ] **Step 4: Запустить import/export RED**

Run: `cargo test --features bundled-sqlite user_dictionary::tests`

Expected: compilation failure because module and APIs do not exist.

- [ ] **Step 5: Реализовать DTO и строгий parser**

Использовать private serde DTO с `#[serde(deny_unknown_fields)]`, `rename_all = "snake_case"`, константами формата/версии/лимитов. Читать через `Read::take(10_MIB + 1)`, отвергать превышение, затем `serde_json::from_slice`. Каждую DTO преобразовать публичными конструкторами `NewUserRule`; внутренние ключи собирать в `HashSet<(Language, String)>`.

- [ ] **Step 6: Реализовать экспорт**

Получить все записи storage-запросом в стабильном порядке, преобразовать в DTO без ID/дат, вызвать `serde_json::to_writer_pretty`, затем записать `\n`. Ошибки I/O и JSON сохранить различимыми.

- [ ] **Step 7: Реализовать импорт одной транзакцией**

После полной валидации открыть transaction. Для каждого ключа выполнить точный lookup внутри transaction и применить `Reject`, `Skip` или `Replace`; при Replace обновить action/replacement/display word, сохранив ID/created_at. Commit только после всех записей. Вернуть точные счётчики.

- [ ] **Step 8: Подтвердить GREEN**

Run: `cargo test --features bundled-sqlite user_dictionary::tests`

Expected: all transfer tests pass.

### Task 4: Приоритет правил в RecognitionRuntime

**Files:**
- Modify: `src/recognition.rs`
- Modify: `src/engine.rs`
- Modify: `tests/dictionaries_and_settings.rs`

**Interfaces:**
- Consumes: `Language`, `normalize_word`, storage list-for-runtime API.
- Produces: runtime index `HashMap<(Language, String), UserRule>` and language-aware rule resolution before exclusions/dictionaries.

- [ ] **Step 1: Написать RED unit-тесты трёх приоритетов**

Для `ConsiderCorrect`, `NeverCorrect` и `AlwaysReplace` создать правила с явным языком и смешать системные словари так, чтобы без правила возникло противоположное решение. Проверить case-insensitive match (`Ghbdtn` matches `ghbdtn`) и точное сохранение replacement.

- [ ] **Step 2: Написать RED runtime integration test**

Сохранить три правила в SQLite, загрузить `RecognitionRuntime`, удалить их из базы и проверить snapshot-семантику и решения всех типов. Сохранить тест глобального `corrections_enabled = false`.

- [ ] **Step 3: Запустить priority RED**

Run: `cargo test --all-targets --features bundled-sqlite user_rule`

Expected: failure because old exact-source lookup ignores language and normalization.

- [ ] **Step 4: Реализовать определение языка и индекс**

В `rules` дать crate-visible `language_of_word` с теми же правилами письменности. В engine загрузить все записи в `HashMap<(Language, String), UserRule>`. В `decide` после глобального флага определить язык/нормализацию и передать максимум одно совпавшее правило. В `decide_correction` сохранить порядок: флаг → пользовательское правило → exclusions → system dictionaries.

- [ ] **Step 5: Подтвердить GREEN и отсутствие регрессий**

Run: `cargo test --all-targets --features bundled-sqlite`

Expected: all unit and integration tests pass, включая URL/e-mail/path/code exclusions.

### Task 5: Документация и полная проверка

**Files:**
- Create: `docs/user-dictionary.md`
- Modify: `docs/mvp.md`
- Modify: `README.md`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: итоговый API и JSON-формат Tasks 1–4.
- Produces: пользовательская документация формата, конфликтов, миграции и runtime-приоритета.

- [ ] **Step 1: Документировать JSON и операции**

В `docs/user-dictionary.md` привести точный пример version 1, таблицы `language`/`type`, условия `replacement`, лимиты, case-insensitive uniqueness, политики Reject/Skip/Replace, атомарность, миграцию v1→v2 и Rust API entry points.

- [ ] **Step 2: Обновить обзорные документы**

В `docs/mvp.md` заменить упоминание схемы v1 на v2 и сослаться на новый документ. В README описать завершённый этап пользовательского словаря и добавить ссылку рядом с системными словарями.

- [ ] **Step 3: Проверить форматирование**

Run: `cargo fmt --check`

Expected: exit 0.

- [ ] **Step 4: Запустить полный набор тестов**

Run: `cargo test --all-targets --features bundled-sqlite`

Expected: exit 0, no failed tests.

- [ ] **Step 5: Запустить линтер всеми features**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: exit 0, no warnings.

- [ ] **Step 6: Проверить итоговый diff**

Run: `git diff --check && git status --short && git diff --stat`

Expected: no whitespace errors; only user-dictionary implementation, tests, dependency lockfile and related docs are changed.
