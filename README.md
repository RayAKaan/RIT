> **V3 status:** See `docs/handoff/FINAL_REPORT.md` (relative to repository root) for current capabilities, blocked storage, and validation; older phase descriptions are historical.

# RIT

RIT is a planned local-first coordination layer for humans and AI agents working on ordinary Git repositories. **Git owns source-code history; RIT owns coordination around it.** RIT is an independent project, not a Git rewrite.

## Status

Early foundation preview (`0.1.0-dev.1`): configuration validation, a reusable core library, a CLI, logging, and runtime diagnostics are implemented. The Phase 2 development branch implements initial Git inspection and experimental SQLite-backed repository registration, without authentication or authorization. Avoid valuable data: the storage gate is failing. Phase 2 is not yet accepted; see its report for untested cases and safety limitations. Identity, workspaces, checkpoints, integration and collaboration have **models and disabled CLI/provider scaffolding only**; sandboxing and privileged operations are not implemented. A minimal MCP discovery-only stdio server, transport-injected SDK foundations, bounded Git history/ref reads, and Tree-sitter static-import extraction over supplied source exist. This is not a production release.

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

The workspace test suite currently fails two mandatory Phase 2.1 security tests; see [status](docs/RIT_STATUS.md).

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

`rit-cli` composes `rit-application`, `rit-git`, `rit-storage`, and `rit-core`; core has no CLI, Git, SQLite, or global logging dependency. [Architecture](docs/architecture/overview.md), [component map](docs/architecture/component-map.md), [testing](docs/development/testing.md), and [decisions](docs/architecture/decisions/0001-foundation.md) describe the current boundaries. Future phases address security, workspaces, awareness, recovery, integration, analysis, interfaces and release; none is implied by this foundation.

## Phase 2 experimental development commands

**Warning:** registry commands are unauthenticated and the storage security gate fails. Use disposable data only; do not use the legacy adoption path on valuable data.

```sh
cargo run --locked -p rit-cli -- doctor --json
cargo run --locked -p rit-cli -- inspect /path/to/existing/repo --json
cargo run --locked -p rit-cli -- --data-dir /outside/repo/rit-data init /path/to/existing/repo
cargo run --locked -p rit-cli -- --data-dir /outside/repo/rit-data repository list --json
cargo run --locked -p rit-cli -- --data-dir /outside/repo/rit-data repository info REPOSITORY_ID
cargo run --locked -p rit-cli -- --data-dir /outside/repo/rit-data status REPOSITORY_ID --json
```

`rit init` registers an **existing** Git repository; it never runs `git init`. Git executable override: `--git-executable` then `RIT_GIT_EXECUTABLE`; data directory override: `--data-dir` then `RIT_DATA_DIR`. Only Unicode paths are currently supported for persistence and status. JSON doctor is now schema version 2: the Phase 1 fields remain and `git` and `storage` were added. Other JSON output uses schema version 1. Git 2.46+ is a provisional requirement; only Git 2.47.3/Linux has been tested. See [Phase 2 report](docs/phases/phase-02-report.md) before relying on any behavior.

## Additional read-only Git commands

`rit repo branches PATH --json`, `rit repo refs PATH --json` and `rit repo log PATH --limit 20 --json` read Git directly without opening the registry. They use local OS-user permissions, not RIT authentication, and their observations can be stale. See `docs/CLI_REFERENCE.md`.
