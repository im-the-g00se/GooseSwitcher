pub(super) const LATEST_SCHEMA_VERSION: i64 = 1;

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
