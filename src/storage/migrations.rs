pub(super) const LATEST_SCHEMA_VERSION: i64 = 2;

pub(super) const MIGRATION_1: &str = r#"
CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE user_rules (
    id          INTEGER PRIMARY KEY,
    source      TEXT NOT NULL UNIQUE,
    action      TEXT NOT NULL CHECK (
        action IN ('consider_correct', 'never_correct', 'always_replace')
    ),
    replacement TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    CHECK (
        (action = 'always_replace' AND replacement IS NOT NULL
            AND length(replacement) > 0)
        OR
        (action <> 'always_replace' AND replacement IS NULL)
    )
);
"#;

pub(super) const MIGRATION_2_CREATE: &str = r#"
CREATE TABLE user_rules_v2 (
    id              INTEGER PRIMARY KEY,
    word            TEXT NOT NULL,
    normalized_word TEXT NOT NULL,
    language        TEXT NOT NULL CHECK (language IN ('ru', 'en')),
    action          TEXT NOT NULL CHECK (
        action IN ('consider_correct', 'never_correct', 'always_replace')
    ),
    replacement     TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (normalized_word, language),
    CHECK (
        (action = 'always_replace' AND replacement IS NOT NULL
            AND length(replacement) > 0)
        OR
        (action <> 'always_replace' AND replacement IS NULL)
    )
);
"#;

pub(super) const MIGRATION_2_FINISH: &str = r#"
DROP TABLE user_rules;
ALTER TABLE user_rules_v2 RENAME TO user_rules;
CREATE INDEX user_rules_filter_order_idx
    ON user_rules (language, action, normalized_word);
"#;
