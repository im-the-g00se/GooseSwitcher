# GooseSwitcher

GooseSwitcher is a planned IBus engine for Fedora Linux that corrects words
typed with the wrong Russian or English keyboard layout.

The current project stage provides the Rust module boundaries and an SQLite
store for settings and user dictionary rules. It does not yet connect to IBus,
recognize words, or provide a GTK settings interface.

## Development on Fedora

Install the build and verification tools:

```shell
sudo dnf install cargo rust sqlite-devel rustfmt clippy
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
