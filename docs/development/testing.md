# Testing

From the repository root, with Rust 1.99.1 and the checked-in lockfile:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace
cargo build --locked --workspace --release
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps
```

CLI tests run the built binary with isolated temporary home/configuration. Tests do not need a network, Git repository or credentials. Test changes in behavior with unit tests and end-to-end subprocess tests; future Git adapter tests must use disposable real Git repositories and record versions. Security and sandbox claims require provider-specific adversarial tests, not mock-only tests. CI must actually pass before claiming platform support.

Phase 2 tests invoke an installed Git executable and create temporary Git repositories and SQLite databases. They do not need a remote or network. Git must be on PATH for these tests. Optional Git SHA-256 support is probed; unsupported builds print a skip message. Platform behavior and security are not certified by a single Linux pass.
