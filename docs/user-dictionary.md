# User dictionary

GooseSwitcher stores user dictionary rules in the same SQLite database as its
settings. Rules are loaded into memory when `RecognitionRuntime` starts, so the
engine does not query SQLite for each completed word. Restart the engine after
changing or importing rules.

## Rule model

Each rule contains a trigger `word`, its `language`, a rule `type`, and an
optional `replacement`:

| JSON value | Rust value | Meaning |
| --- | --- | --- |
| `ru` | `Language::Russian` | The trigger uses Russian Cyrillic letters. |
| `en` | `Language::English` | The trigger uses English ASCII letters. |
| `consider_correct` | `RuleAction::ConsiderCorrect` | Treat the trigger as a correct word. |
| `never_correct` | `RuleAction::NeverCorrect` | Never correct the trigger automatically. |
| `always_replace` | `RuleAction::AlwaysReplace` | Replace the trigger with `replacement`. |

The language describes the trigger's written script, not the replacement's
language. For example, `ghbdtn` → `привет` is an English trigger because its
letters are English.

Words are matched without regard to case. The pair of normalized word and
language is unique, so `Rust` and `rust` conflict in the English dictionary.
Words may contain up to 128 letters and internal hyphens or apostrophes. Mixed
scripts, whitespace, numbers, leading/trailing joiners, and repeated joiners
are rejected.

Replacement text is required only for `always_replace`. It may contain up to
1024 Unicode characters and multiple words, but it cannot have leading or
trailing whitespace or contain control characters.

## Storage API

`SqliteStore` exposes operations suitable for a settings application:

```rust
create_rule(NewUserRule) -> Result<UserRule, StorageError>
update_rule(id, NewUserRule) -> Result<Option<UserRule>, StorageError>
get_rule(id) -> Result<Option<UserRule>, StorageError>
find_rule(language, word) -> Result<Option<UserRule>, StorageError>
search_rules(&RuleSearch) -> Result<Vec<UserRule>, StorageError>
delete_rule(id) -> Result<bool, StorageError>
```

Creation never overwrites an existing normalized key. Updating preserves the
record ID and creation timestamp. Search supports a case-insensitive word
substring, language and action filters, plus stable pagination. Page size must
be from 1 through 500 and defaults to 100.

The SQLite schema version is 2. Opening a version 1 database migrates existing
rules transactionally, inferring their language from the trigger script while
preserving IDs and timestamps. An invalid legacy rule or a case-insensitive
duplicate aborts the migration and leaves the version 1 database unchanged.

## JSON format version 1

Export uses UTF-8, deterministic ordering, pretty formatting, and a final
newline. Database IDs and timestamps are deliberately omitted because they are
local metadata.

```json
{
  "format": "gooseswitcher-user-dictionary",
  "version": 1,
  "rules": [
    {
      "word": "ghbdtn",
      "language": "en",
      "type": "always_replace",
      "replacement": "привет"
    },
    {
      "word": "Rust",
      "language": "en",
      "type": "consider_correct"
    }
  ]
}
```

The envelope and every rule reject unknown fields. Unknown format versions,
languages, actions, invalid rule combinations, and duplicate normalized keys
inside the document are errors. Imports are limited to 10 MiB and 10,000
rules.

Use `user_dictionary::export_json` and `user_dictionary::import_json` with any
`Write` or `Read` implementation. Import fully parses and validates the
document before changing SQLite, then applies all changes in one transaction.

`ImportConflictPolicy` controls keys already in the database:

- `Reject` (and the enum default) aborts the entire import;
- `Skip` preserves existing rules and imports non-conflicting rules;
- `Replace` updates existing rule content while preserving its ID and creation
  timestamp.

A successful import returns `ImportReport { created, updated, skipped }`.

## Recognition priority

The runtime evaluates a completed word in this order:

1. global `corrections_enabled` setting;
2. matching user rule by language and normalized word;
3. exclusions for URL, e-mail, path, number, and code-like tokens;
4. existing conservative system-dictionary decision.

All three user rule types therefore take priority over system dictionaries.
The global switch still disables every automatic action, including
`always_replace`.
