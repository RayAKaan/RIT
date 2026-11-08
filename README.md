# RIT

RIT is a planned local-first coordination layer for humans and AI agents working on ordinary Git repositories. **Git owns source-code history; RIT owns coordination around it.** RIT is an independent project, not a Git rewrite.

## Status

Early foundation preview (`0.1.0-dev.1`): configuration validation, a reusable core library, a CLI, logging, and runtime diagnostics are implemented. Repository management, identity, workspaces, sandboxing, checkpoints, integration, analysis, MCP and SDKs are **not implemented**. This is not a production release.

## Build and run

The tested environment is Linux x86-64 with stable Rust 1.99.1. macOS and Windows are CI candidates, not confirmed supported platforms until their checks run. Install Rust using rustup, then from the project root:

```sh
cargo build --locked --workspace
cargo run --locked -p rit-cli -- --help
cargo run --locked -p rit-cli -- --version
cargo run --locked -p rit-cli -- doctor
cargo run --locked -p rit-cli -- doctor --json
cargo run --locked -p rit-cli -- config validate
cargo test --locked --workspace --all-targets
```

`cargo build` produces `target/debug/rit` (Windows: `target\debug\rit.exe`). No global install is required.

## Configuration

Settings are resolved in this order: built-in defaults, optional user file, `RIT_LOG_LEVEL` and `RIT_LOG_FORMAT`, then `--log-level` and `--log-format`. The default user file is `$XDG_CONFIG_HOME/rit/config.toml` when `XDG_CONFIG_HOME` is absolute, otherwise `$HOME/.config/rit/config.toml` on non-Windows; on Windows it is `%APPDATA%\rit\config.toml`. If no base path is available, defaults are used. A missing default file is allowed; an explicitly selected `--config PATH` must exist and be readable. An invalid file or override is an error even if a higher-precedence value might replace it. Unknown fields and schema versions are rejected. The tool never writes configuration.

```toml
schema_version = 1
log_level = "info" # error, warn, info, debug, trace
log_format = "text" # text or json
```

Configuration has no secret fields. Do not place credentials in it. Logs go to stderr; `doctor --json` outputs a versioned JSON object on stdout on success. On invalid configuration it reports a safe error on stderr and exits nonzero rather than emitting a success JSON document. Exit codes: 0 success, 2 CLI usage, 3 configuration, 4 runtime/output, 5 internal initialization. Version/help are handled before configuration loading.

## Architecture and roadmap

`rit-cli` depends on `rit-core`; core has no CLI dependency or side effects on global logging. [Architecture](docs/architecture/overview.md), [component map](docs/architecture/component-map.md), [testing](docs/development/testing.md), and [decisions](docs/architecture/decisions/0001-foundation.md) describe the current boundaries. Future phases address Git inspection, security, workspaces, awareness, recovery, integration, analysis, interfaces and release; none is implied by this foundation.
