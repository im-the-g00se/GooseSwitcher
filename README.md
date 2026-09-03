# GooseSwitcher

GooseSwitcher is a planned IBus engine for Fedora Linux that corrects words
typed with the wrong Russian or English keyboard layout.

The current project stage provides the Rust module boundaries, an SQLite store
for settings and a complete RU/EN user dictionary, plus a pure layout
recognition engine. User rules support validated CRUD and search, transactional
schema migration, and strict versioned JSON import/export. At startup the
engine loads Fedora's Russian and US English Hunspell dictionaries, user rules,
and recognition settings into memory. It applies user rules before system
dictionaries without querying SQLite for each word. It does not yet connect to
IBus or provide a GTK settings interface.

## Development on Fedora

Install the build and verification tools:

```shell
sudo dnf install cargo rust sqlite-devel rustfmt clippy hunspell-en-US hunspell-ru
```

Run the checks:

```shell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

The default build links to Fedora's system SQLite. In a local or CI environment
without `sqlite-devel`, the explicitly opt-in bundled SQLite feature can be used:

```shell
cargo test --all-targets --features bundled-sqlite
```

See [System dictionaries and recognition settings](docs/dictionaries.md) for
the dictionary sources, update procedure, startup behavior, and persisted
setting keys. See [User dictionary](docs/user-dictionary.md) for rule
validation, storage APIs, migration behavior, conflict policies, and the JSON
format.
