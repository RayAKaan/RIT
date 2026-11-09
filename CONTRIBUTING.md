# Contributing

Use Rust 1.99.1 through rustup; `rust-toolchain.toml` requests rustfmt and clippy. See [setup](docs/development/setup.md) and [testing](docs/development/testing.md). Work on a topic branch; do not commit directly to the default branch. Phase 1 uses `phase/01-foundation`; later work should follow the repository's adopted policy. Write clear, scoped commit messages. Do not force-push or rewrite shared history.

Before a PR, run the commands in the testing guide with `--locked`, explain behavior and security effects, link relevant decisions, include tests and documentation, and resolve mandatory CI failures. Review and merge require repository authorization and passing required checks. No branch protection or reviewer configuration is asserted by this document.
