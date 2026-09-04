# Settings Interface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Добавить нативное GTK4/libadwaita-приложение настроек GooseSwitcher поверх существующих SQLite, правил и JSON API.

**Architecture:** GTK-независимый `SettingsService` владеет `SqliteStore` и является единственной границей данных для UI. Опциональный feature `settings-ui` добавляет отдельный бинарник и тонкий GTK-слой; ядро и его тесты продолжают собираться без графических библиотек.

**Tech Stack:** Rust 2021, rusqlite, serde/serde_json, gtk4 0.11.4 (GTK 4.10 API), libadwaita 0.9.2 (libadwaita 1.4 API), IBus CLI для read-only диагностики.

**Spec:** `docs/superpowers/specs/2026-09-04-settings-interface-design.md`

## Global Constraints

- Поддерживать только RU и EN.
- Не использовать глобальный перехват клавиатуры.
- Не реализовывать IBus-движок, обработку горячих клавиш, systemd user service или RPM-упаковку.
- Не менять алгоритм распознавания или JSON-формат пользовательского словаря версии 1.
- Автозамена и минимальная длина используют существующие ключи `corrections_enabled` и `minimum_word_length`; значения по умолчанию — `true` и `3`.
- Минимальная длина в UI и прикладном сервисе ограничена диапазоном `1..=64`.
- Исключённые приложения хранятся под ключом `excluded_applications` как JSON-массив уникальных строк длиной не более 255 Unicode-символов без управляющих символов.
- Ядро не получает обязательной зависимости от GTK; GUI доступен только с feature `settings-ui`.
- GTK-виджеты не дублируют валидацию `NewUserRule`, SQLite CRUD или импорт/экспорт JSON.
- Пользовательские сообщения интерфейса пишутся по-русски и не скрывают техническую причину ошибки.

---

### Task 1: Типизированная модель основных настроек

**Files:**
- Modify: `src/config.rs`
- Replace: `src/settings_ui.rs`
- Create: `src/settings_ui/preferences.rs`
- Test: `src/settings_ui/preferences.rs`

**Interfaces:**
- Consumes: `SqliteStore::{get_setting,set_setting}`, `CORRECTIONS_ENABLED`, `MINIMUM_WORD_LENGTH`.
- Produces: `EXCLUDED_APPLICATIONS`, `Preferences`, `PreferencesError`, `load_preferences`, `save_corrections_enabled`, `save_minimum_word_length`, `MINIMUM_WORD_LENGTH_RANGE`.

- [ ] **Step 1: Написать падающие тесты значений по умолчанию и чтения SQLite**

Добавить в `preferences.rs` тесты с временной настоящей базой:

```rust
#[test]
fn preferences_use_defaults_and_read_stored_values() {
    let (_directory, mut store) = store();
    assert_eq!(load_preferences(&store).unwrap(), Preferences::default());

    store.set_setting(CORRECTIONS_ENABLED, "false").unwrap();
    store.set_setting(MINIMUM_WORD_LENGTH, "7").unwrap();

    assert_eq!(
        load_preferences(&store).unwrap(),
        Preferences { corrections_enabled: false, minimum_word_length: 7 }
    );
}
```

- [ ] **Step 2: Запустить тест и подтвердить RED**

Run: `cargo test settings_ui::preferences::tests::preferences_use_defaults_and_read_stored_values -- --exact`

Expected: FAIL, потому что `Preferences` и `load_preferences` ещё не существуют.

- [ ] **Step 3: Реализовать минимальную модель и чтение**

В `config.rs` добавить:

```rust
pub const EXCLUDED_APPLICATIONS: &str = "excluded_applications";
```

В `settings_ui.rs` объявить модули и re-export. В `preferences.rs` реализовать:

```rust
pub const MINIMUM_WORD_LENGTH_RANGE: RangeInclusive<usize> = 1..=64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub corrections_enabled: bool,
    pub minimum_word_length: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { corrections_enabled: true, minimum_word_length: 3 }
    }
}

pub fn load_preferences(store: &SqliteStore) -> Result<Preferences, PreferencesError>;
```

Разбирать bool только из `"true"`/`"false"`, integer через `parse::<usize>()`, затем проверять диапазон.

- [ ] **Step 4: Добавить RED-тесты повреждённых значений**

```rust
#[test]
fn preferences_reject_malformed_and_out_of_range_values() {
    for (key, value) in [
        (CORRECTIONS_ENABLED, "yes"),
        (MINIMUM_WORD_LENGTH, "word"),
        (MINIMUM_WORD_LENGTH, "0"),
        (MINIMUM_WORD_LENGTH, "65"),
    ] {
        let (_directory, mut store) = store();
        store.set_setting(key, value).unwrap();
        assert!(matches!(
            load_preferences(&store),
            Err(PreferencesError::InvalidSetting { key: ref found, value: ref raw })
                if found == key && raw == value
        ));
    }
}
```

- [ ] **Step 5: Запустить тест повреждённых значений и подтвердить RED**

Run: `cargo test settings_ui::preferences::tests::preferences_reject_malformed_and_out_of_range_values -- --exact`

Expected: FAIL до добавления `PreferencesError::InvalidSetting` и полной проверки диапазона.

- [ ] **Step 6: Реализовать ошибки и сохранение значений**

Определить `PreferencesError::{Storage, InvalidSetting, MinimumWordLengthOutOfRange}` с `Display`, `Error::source` и `From<StorageError>`. Реализовать:

```rust
pub fn save_corrections_enabled(
    store: &mut SqliteStore,
    enabled: bool,
) -> Result<(), PreferencesError>;

pub fn save_minimum_word_length(
    store: &mut SqliteStore,
    length: usize,
) -> Result<(), PreferencesError>;
```

Сохранять только строки `true`/`false` и десятичное число; значение вне диапазона отклонять до SQLite.

- [ ] **Step 7: Проверить GREEN и весь модуль**

Run: `cargo test settings_ui::preferences::tests`

Expected: PASS, включая дополнительный тест точных строк, прочитанных через `get_setting`.

- [ ] **Step 8: Commit**

```bash
git add src/config.rs src/settings_ui.rs src/settings_ui/preferences.rs
git commit -m "feat: add typed settings preferences"
```

---

### Task 2: Модель списка исключённых приложений

**Files:**
- Create: `src/settings_ui/excluded_apps.rs`
- Modify: `src/settings_ui.rs`
- Test: `src/settings_ui/excluded_apps.rs`

**Interfaces:**
- Consumes: `EXCLUDED_APPLICATIONS`, `SqliteStore::{get_setting,set_setting}`.
- Produces: `ExcludedApplications`, `ApplicationIdError`, `ExcludedApplicationsError`, `load_excluded_applications`, `save_excluded_applications`.

- [ ] **Step 1: Написать RED-тест валидации и уникальности**

```rust
#[test]
fn application_ids_are_trimmed_deduplicated_and_keep_order() {
    let apps = ExcludedApplications::new([
        "  org.gnome.Terminal  ",
        "firefox",
        "org.gnome.Terminal",
    ]).unwrap();

    assert_eq!(apps.as_slice(), ["org.gnome.Terminal", "firefox"]);
}

#[test]
fn application_ids_reject_empty_long_and_control_text() {
    assert!(matches!(ExcludedApplications::new(["  "]), Err(ApplicationIdError::Empty)));
    assert!(matches!(ExcludedApplications::new(["a\n"]), Err(ApplicationIdError::ControlCharacter)));
    assert!(matches!(ExcludedApplications::new(["a".repeat(256)]), Err(ApplicationIdError::TooLong { max: 255 })));
}
```

- [ ] **Step 2: Запустить и подтвердить RED**

Run: `cargo test settings_ui::excluded_apps::tests`

Expected: compile FAIL, модель отсутствует. Если фильтр не выбирает оба теста, запускать `cargo test settings_ui::excluded_apps::tests`.

- [ ] **Step 3: Реализовать валидированную коллекцию**

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExcludedApplications(Vec<String>);

impl ExcludedApplications {
    pub fn new<I, S>(values: I) -> Result<Self, ApplicationIdError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>;
    pub fn as_slice(&self) -> &[String];
    pub fn add(&mut self, value: impl Into<String>) -> Result<bool, ApplicationIdError>;
    pub fn remove(&mut self, value: &str) -> bool;
}
```

Сначала отклонять управляющие символы, затем применять `trim`; это не позволяет
молча превратить ввод с переводом строки в допустимый ID. Использовать
`HashSet<String>` только для обнаружения точных дубликатов; результирующий `Vec`
сохраняет порядок.

- [ ] **Step 4: Написать RED-тест SQLite JSON round-trip и повреждения**

```rust
#[test]
fn excluded_applications_round_trip_as_stable_json() {
    let (_directory, mut store) = store();
    let apps = ExcludedApplications::new(["org.gnome.Terminal", "firefox"]).unwrap();
    save_excluded_applications(&mut store, &apps).unwrap();

    assert_eq!(
        store.get_setting(EXCLUDED_APPLICATIONS).unwrap().unwrap().value,
        r#"["org.gnome.Terminal","firefox"]"#
    );
    assert_eq!(load_excluded_applications(&store).unwrap(), apps);
}

#[test]
fn malformed_stored_application_json_is_reported() {
    let (_directory, mut store) = store();
    store.set_setting(EXCLUDED_APPLICATIONS, "{}").unwrap();
    assert!(matches!(
        load_excluded_applications(&store),
        Err(ExcludedApplicationsError::InvalidStoredJson { .. })
    ));
}
```

- [ ] **Step 5: Запустить тесты и подтвердить RED**

Run: `cargo test settings_ui::excluded_apps::tests`

Expected: новые round-trip тесты FAIL до функций хранения.

- [ ] **Step 6: Реализовать JSON-хранение и ошибки**

Отсутствующий ключ означает пустой список. Десериализовать только `Vec<String>`, затем обязательно прогонять через `ExcludedApplications::new`. Ошибки различать как `Storage`, `InvalidStoredJson` и `InvalidApplicationId`.

- [ ] **Step 7: Проверить GREEN**

Run: `cargo test settings_ui::excluded_apps::tests`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/settings_ui.rs src/settings_ui/excluded_apps.rs
git commit -m "feat: store excluded applications"
```

---

### Task 3: Единый прикладной сервис UI

**Files:**
- Create: `src/settings_ui/service.rs`
- Modify: `src/settings_ui.rs`
- Test: `src/settings_ui/service.rs`

**Interfaces:**
- Consumes: Task 1/2 functions, `SqliteStore`, `RuleSearch`, `NewUserRule`, `UserRule`, `ImportConflictPolicy`, `ImportReport`, `import_json`, `export_json`.
- Produces: `SettingsService`, `SettingsServiceError` and the exact public methods below.

- [ ] **Step 1: Написать RED-тест делегирования CRUD**

```rust
#[test]
fn service_creates_updates_searches_and_deletes_rules() {
    let (_directory, store) = store();
    let mut service = SettingsService::from_store(store);
    let created = service.create_rule(
        NewUserRule::consider_correct("Rust", Language::English).unwrap()
    ).unwrap();
    let updated = service.update_rule(
        created.id,
        NewUserRule::never_correct("Rust", Language::English).unwrap()
    ).unwrap().unwrap();

    assert_eq!(updated.action, RuleAction::NeverCorrect);
    assert_eq!(service.search_rules(&RuleSearch::default()).unwrap(), [updated]);
    assert!(service.delete_rule(created.id).unwrap());
}
```

- [ ] **Step 2: Запустить и подтвердить RED**

Run: `cargo test settings_ui::service::tests::service_creates_updates_searches_and_deletes_rules -- --exact`

Expected: FAIL, `SettingsService` отсутствует.

- [ ] **Step 3: Реализовать сервис и его точный API**

```rust
pub struct SettingsService { store: SqliteStore }

impl SettingsService {
    pub fn open_default() -> Result<Self, SettingsServiceError>;
    pub fn from_store(store: SqliteStore) -> Self;
    pub fn preferences(&self) -> Result<Preferences, SettingsServiceError>;
    pub fn set_corrections_enabled(&mut self, enabled: bool) -> Result<(), SettingsServiceError>;
    pub fn set_minimum_word_length(&mut self, length: usize) -> Result<(), SettingsServiceError>;
    pub fn excluded_applications(&self) -> Result<ExcludedApplications, SettingsServiceError>;
    pub fn save_excluded_applications(&mut self, apps: &ExcludedApplications) -> Result<(), SettingsServiceError>;
    pub fn search_rules(&self, search: &RuleSearch) -> Result<Vec<UserRule>, SettingsServiceError>;
    pub fn create_rule(&mut self, rule: NewUserRule) -> Result<UserRule, SettingsServiceError>;
    pub fn update_rule(&mut self, id: i64, rule: NewUserRule) -> Result<Option<UserRule>, SettingsServiceError>;
    pub fn delete_rule(&mut self, id: i64) -> Result<bool, SettingsServiceError>;
    pub fn import_dictionary(&mut self, path: &Path, policy: ImportConflictPolicy) -> Result<ImportReport, SettingsServiceError>;
    pub fn export_dictionary(&self, path: &Path) -> Result<(), SettingsServiceError>;
}
```

`SettingsServiceError` различает `Preferences`, `ExcludedApplications`, `Storage`, `DictionaryTransfer` и `OpenFile`/`CreateFile` с путём. Все варианты реализуют `source`.

- [ ] **Step 4: Написать RED-тесты файлового импорта/экспорта и политик**

Создать конфликтующее правило в базе и JSON во временном файле. Проверить:

```rust
assert!(service.import_dictionary(&path, ImportConflictPolicy::Reject).is_err());
assert_eq!(
    service.import_dictionary(&path, ImportConflictPolicy::Skip).unwrap().skipped,
    1
);
assert_eq!(
    service.import_dictionary(&path, ImportConflictPolicy::Replace).unwrap().updated,
    1
);
service.export_dictionary(&export_path).unwrap();
assert!(fs::read_to_string(export_path).unwrap().ends_with("\n"));
```

- [ ] **Step 5: Запустить новые тесты и подтвердить RED**

Run: `cargo test settings_ui::service::tests`

Expected: transfer-тесты FAIL до реализации файловых методов.

- [ ] **Step 6: Реализовать файловые методы через существующий JSON API**

Открывать `File`, затем вызывать `import_json`/`export_json`; не читать JSON и не разбирать конфликты повторно в сервисе.

- [ ] **Step 7: Проверить GREEN**

Run: `cargo test settings_ui::service::tests`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/settings_ui.rs src/settings_ui/service.rs
git commit -m "feat: add settings application service"
```

---

### Task 4: Read-only диагностика состояния IBus

**Files:**
- Create: `src/settings_ui/ibus_status.rs`
- Modify: `src/settings_ui.rs`
- Test: `src/settings_ui/ibus_status.rs`

**Interfaces:**
- Consumes: output of `ibus list-engine` and `ibus engine`.
- Produces: `IBUS_ENGINE_ID`, `IbusStatus`, `CommandResult`, `interpret_ibus_status`, `probe_ibus_status`.

- [ ] **Step 1: Написать RED-тесты четырёх состояний**

```rust
#[test]
fn status_is_active_when_registered_engine_is_selected() {
    assert_eq!(
        interpret_ibus_status(
            Ok(CommandResult::success("language: Other\n  gooseswitcher - GooseSwitcher\n")),
            Ok(CommandResult::success("gooseswitcher\n")),
        ),
        IbusStatus::Active,
    );
}

#[test]
fn status_distinguishes_inactive_missing_and_unavailable() {
    assert_eq!(registered_with("xkb:ru::rus"), IbusStatus::RegisteredInactive);
    assert_eq!(not_registered(), IbusStatus::NotRegistered);
    assert!(matches!(command_failed(), IbusStatus::Unavailable { .. }));
}
```

Тестовые helper-функции должны передавать полные `CommandResult`, а не mock-объекты.

- [ ] **Step 2: Запустить и подтвердить RED**

Run: `cargo test settings_ui::ibus_status::tests`

Expected: FAIL, типы и интерпретатор отсутствуют.

- [ ] **Step 3: Реализовать чистый интерпретатор**

```rust
pub const IBUS_ENGINE_ID: &str = "gooseswitcher";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IbusStatus {
    Active,
    RegisteredInactive,
    NotRegistered,
    Unavailable { reason: String },
}

pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn interpret_ibus_status(
    listed: io::Result<CommandResult>,
    current: io::Result<CommandResult>,
) -> IbusStatus;
```

Из `list-engine` извлекать первый токен каждой строки, начинающейся с пробела; сравнивать точный ID, а не подстроку. Ошибка запуска или ненулевой exit превращается в `Unavailable` с stderr/IO причиной.

- [ ] **Step 4: Реализовать реальный probe и проверить GREEN**

```rust
pub fn probe_ibus_status() -> IbusStatus {
    interpret_ibus_status(run_ibus(&["list-engine"]), run_ibus(&["engine"]))
}
```

`run_ibus` использует `std::process::Command`, `String::from_utf8_lossy` и не меняет состояние IBus.

Run: `cargo test settings_ui::ibus_status::tests`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/settings_ui.rs src/settings_ui/ibus_status.rs
git commit -m "feat: report IBus engine status"
```

---

### Task 5: Опциональный GUI-бинарник и каркас окна

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `src/bin/gooseswitcher-settings.rs`
- Create: `src/settings_ui/gtk_app.rs`
- Modify: `src/settings_ui.rs`

**Interfaces:**
- Consumes: `SettingsService::open_default`, `probe_ibus_status`, `Preferences`.
- Produces: Cargo feature `settings-ui`, binary `gooseswitcher-settings`, `gtk_app::run() -> glib::ExitCode`.

- [ ] **Step 1: Добавить декларативную конфигурацию feature и проверить ожидаемую инфраструктурную ошибку**

В `Cargo.toml` добавить:

```toml
[features]
default = []
bundled-sqlite = ["rusqlite/bundled"]
settings-ui = ["dep:gtk", "dep:adw"]

[dependencies]
gtk = { package = "gtk4", version = "0.11.4", features = ["v4_10"], optional = true }
adw = { package = "libadwaita", version = "0.9.2", features = ["v1_4", "gtk_v4_10"], optional = true }

[[bin]]
name = "gooseswitcher-settings"
path = "src/bin/gooseswitcher-settings.rs"
required-features = ["settings-ui"]
```

Run: `cargo check --features settings-ui --bin gooseswitcher-settings`

Expected in the current container: FAIL from `pkg-config` because `gtk4.pc`/`libadwaita-1.pc` are absent. Expected on a prepared Fedora host: initial compile FAIL because the binary source does not exist. Record the exact result; this configuration step is the TDD exception for generated Cargo metadata approved by the specification.

- [ ] **Step 2: Создать минимальный binary entry point и GTK application**

```rust
fn main() -> glib::ExitCode {
    gooseswitcher::settings_ui::gtk_app::run()
}
```

`gtk_app::run` creates `adw::Application` with ID `io.github.gooseswitcher.Settings`, connects `activate`, and either creates the preferences window with `SettingsService::open_default()` or shows a modal error window without panic.

- [ ] **Step 3: Построить четыре пустые нативные страницы и основные настройки**

Создать `adw::PreferencesWindow`, pages «Основные», «Словарь», «Приложения», «IBus». На основной странице добавить `SwitchRow`, `SpinRow` с `Adjustment(3, 1, 64, 1, 5, 0)`, explanation row и toast overlay. Сигналы вызывают `SettingsService`; при ошибке откатывают widget к последнему сохранённому значению с guard-флагом против рекурсии.

- [ ] **Step 4: Добавить страницу статуса IBus**

Преобразовать четыре `IbusStatus` в русские заголовки/описания и semantic icon names. Кнопка «Обновить» повторно вызывает `probe_ibus_status`. Добавить точную инструкцию активации и уведомление об отсутствующей в этом этапе регистрации/горячих клавишах.

- [ ] **Step 5: Проверить core build и GUI compile**

Run: `cargo check --all-targets`

Expected: PASS без GTK development-пакетов.

Run on Fedora with `gtk4-devel libadwaita-devel`: `cargo check --features settings-ui --bin gooseswitcher-settings`

Expected: PASS. В текущем контейнере допустим только ранее зафиксированный `pkg-config` blocker; Rust-синтаксис дополнительно проверять `cargo fmt --check` и повторять feature-check после доступной установки пакетов.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/bin/gooseswitcher-settings.rs src/settings_ui.rs src/settings_ui/gtk_app.rs
git commit -m "feat: add native settings application shell"
```

---

### Task 6: Редактор пользовательского словаря

**Files:**
- Create: `src/settings_ui/gtk_app/rule_dialog.rs`
- Create: `src/settings_ui/gtk_app/dictionary_page.rs`
- Modify: `src/settings_ui/gtk_app.rs`

**Interfaces:**
- Consumes: Task 3 CRUD/transfer methods, `RuleSearch`, `Language`, `RuleAction`, `NewUserRule`, `UserRule`, `ImportConflictPolicy`.
- Produces: `dictionary_page::build(Rc<RefCell<SettingsService>>, &adw::PreferencesWindow) -> adw::PreferencesPage`; `rule_dialog::present(...)` callback returning saved rule or cancellation.

- [ ] **Step 1: Реализовать testable presentation mappings before widgets**

В `dictionary_page.rs` добавить unit-тесты GTK-independent helper-функций под feature:

```rust
#[test]
fn rule_action_labels_are_complete() {
    assert_eq!(action_label(RuleAction::ConsiderCorrect), "Считать корректным");
    assert_eq!(action_label(RuleAction::NeverCorrect), "Никогда не исправлять");
    assert_eq!(action_label(RuleAction::AlwaysReplace), "Всегда заменять");
}
```

Run on prepared Fedora: `cargo test --features settings-ui settings_ui::gtk_app::dictionary_page::tests`

Expected: FAIL до helper-функций.

- [ ] **Step 2: Реализовать список, поиск и фильтры**

Страница содержит `SearchEntry`, два `ComboRow`, кнопки add/import/export и `PreferencesGroup` результатов. Каждое изменение строит:

```rust
RuleSearch {
    text: non_empty_search_text,
    language: selected_language,
    action: selected_action,
    limit: 500,
    offset: 0,
}
```

Полностью перестраивать строки из ответа сервиса. Строка показывает слово, язык, action label и replacement; activation opens edit dialog. Пустой результат показывает `StatusPage`.

- [ ] **Step 3: Реализовать единый add/edit dialog**

Диалог содержит `EntryRow` слова, `ComboRow` языка/action и `EntryRow` замены. Поле замены visible только для `AlwaysReplace`. При Save построить одну из публичных фабрик `NewUserRule::{consider_correct,never_correct,always_replace}` и показать локализованный `RuleValidationError` возле релевантного поля. После успешного create/update закрыть диалог и reload list.

- [ ] **Step 4: Реализовать подтверждаемое удаление**

Добавить destructive button в edit dialog. `AlertDialog` называет слово; только ответ «Удалить» вызывает `delete_rule`. Ошибка оставляет диалог открытым, успешное удаление reloads list.

- [ ] **Step 5: Реализовать нативный import/export**

Использовать `gtk::FileDialog` и JSON `FileFilter`. Перед импортом `AlertDialog` предлагает три явных политики `Reject`, `Skip`, `Replace`; выбранная политика передаётся без изменения в сервис. Успех показывает toast со всеми тремя счётчиками. Отмена `FileDialog` игнорируется только для `gtk::DialogError::Dismissed`; остальные ошибки отображаются.

- [ ] **Step 6: Проверить compile и mappings**

Run on prepared Fedora: `cargo test --features settings-ui settings_ui::gtk_app::dictionary_page::tests`

Expected: PASS.

Run on prepared Fedora: `cargo check --features settings-ui --bin gooseswitcher-settings`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/settings_ui/gtk_app.rs src/settings_ui/gtk_app/rule_dialog.rs src/settings_ui/gtk_app/dictionary_page.rs
git commit -m "feat: add user dictionary editor"
```

---

### Task 7: Редактор исключённых приложений

**Files:**
- Create: `src/settings_ui/gtk_app/applications_page.rs`
- Modify: `src/settings_ui/gtk_app.rs`

**Interfaces:**
- Consumes: `SettingsService::{excluded_applications,save_excluded_applications}`, `ExcludedApplications::{add,remove,as_slice}`.
- Produces: `applications_page::build(Rc<RefCell<SettingsService>>, &adw::PreferencesWindow) -> adw::PreferencesPage`.

- [ ] **Step 1: Написать RED-тест преобразования результата add**

В GTK-independent helper проверить, что duplicate даёт информационное сообщение, invalid ID — field error, new value — reload:

```rust
#[test]
fn add_outcome_has_distinct_user_feedback() {
    assert_eq!(feedback_for_add(false), AddFeedback::AlreadyExists);
    assert_eq!(feedback_for_add(true), AddFeedback::Saved);
}
```

Run on prepared Fedora: `cargo test --features settings-ui settings_ui::gtk_app::applications_page::tests`

Expected: FAIL до helper types.

- [ ] **Step 2: Реализовать страницу и добавление**

Добавить explanatory group, `EntryRow` с примерами `org.gnome.Terminal`/`firefox`, кнопку «Добавить» и список action rows. On add: load collection, call `add`, save only when returned `true`, clear field after success. Validation error is displayed as row error/subtitle and input is preserved.

- [ ] **Step 3: Реализовать удаление с rollback by reload**

Delete button opens confirmation. On confirm: reload collection from SQLite, remove exact ID, save, rebuild list. On storage error keep old displayed rows and show toast.

- [ ] **Step 4: Проверить feature compile**

Run on prepared Fedora: `cargo test --features settings-ui settings_ui::gtk_app::applications_page::tests`

Expected: PASS.

Run on prepared Fedora: `cargo check --features settings-ui --bin gooseswitcher-settings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/settings_ui/gtk_app.rs src/settings_ui/gtk_app/applications_page.rs
git commit -m "feat: add excluded applications editor"
```

---

### Task 8: Документация, форматирование и полная проверка

**Files:**
- Modify: `README.md`
- Modify: `docs/mvp.md`
- Modify: `docs/dictionaries.md`

**Interfaces:**
- Consumes: итоговые команды сборки и фактические ограничения UI.
- Produces: пользовательская инструкция установки dev-пакетов, запуска UI, описания persisted key и текущих IBus/hotkey/exclusion ограничений.

- [ ] **Step 1: Обновить README**

Заменить утверждение об отсутствии GTK-интерфейса. Добавить Fedora dependencies:

```shell
sudo dnf install cargo rust sqlite-devel rustfmt clippy \
  gtk4-devel libadwaita-devel hunspell-en-US hunspell-ru
```

Добавить запуск:

```shell
cargo run --features settings-ui --bin gooseswitcher-settings
```

Явно указать: настройки и правила применяются после перезапуска будущего IBus engine; RPM и регистрация IBus ещё не реализованы.

- [ ] **Step 2: Обновить docs/mvp.md и docs/dictionaries.md**

Описать `excluded_applications` как JSON setting, диапазон минимальной длины UI `1..=64`, отсутствие работающих hotkeys/IBus adapter и наличие отдельного settings binary. Не описывать исключения как уже применяемые движком.

- [ ] **Step 3: Запустить форматирование с исправлением и проверкой**

Run: `cargo fmt --all`

Run: `cargo fmt --all --check`

Expected: PASS.

- [ ] **Step 4: Запустить все headless tests**

Run: `cargo test --all-targets`

Expected: PASS, 0 failed.

Run: `cargo test --all-targets --features bundled-sqlite`

Expected: PASS, 0 failed.

- [ ] **Step 5: Запустить линтер ядра**

Run: `cargo clippy --all-targets -- -D warnings`

Expected: PASS, 0 warnings.

- [ ] **Step 6: Запустить полную GUI-проверку**

Run on Fedora with dev packages: `cargo check --features settings-ui --bin gooseswitcher-settings`

Run on Fedora with dev packages: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: PASS, 0 warnings. Если текущая машина всё ещё не имеет `gtk4.pc` или `libadwaita-1.pc`, сохранить полный `pkg-config` вывод как единственный инфраструктурный blocker и не заявлять GUI build успешным.

- [ ] **Step 7: Проверить требования и diff**

Run: `git diff --check`

Run: `git status --short`

Вручную сопоставить diff с каждым пунктом спецификации: общие настройки, словарь CRUD/search/import/export, приложения, IBus status/instruction, ошибки, отсутствие RPM и отсутствие реализации hotkeys/engine.

- [ ] **Step 8: Commit**

```bash
git add README.md docs/mvp.md docs/dictionaries.md
git commit -m "docs: describe settings application"
```
