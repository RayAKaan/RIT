> **V3 status:** See `docs/handoff/FINAL_REPORT.md` (relative to repository root) for current capabilities, blocked storage, and validation; older phase descriptions are historical.

# RIT

RIT is a planned local-first coordination layer for humans and AI agents working on ordinary Git repositories. **Git owns source-code history; RIT owns coordination around it.** RIT is an independent project, not a Git rewrite.

## Status

Early development preview (`0.1.0-dev.1`), **not production ready**. V3 adds registry-independent Git inspection, partial committed-source dependency analysis, ephemeral Git activity polling, explicit Linux commit-only checkpoints, and opt-in scoped read-only stdio MCP. Managed workspaces, a real notification watcher, complete checkpoints/restoration, conflict simulation, integration, identity and authorization remain unavailable. The Phase 2.1 storage gate is blocked: writable registry and legacy adoption fail closed at the library boundary, and the experimental read-only registry is unauthenticated. See [the V3 handoff](docs/handoff/FINAL_REPORT.md).
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

The full workspace test suite currently has 21 failures after the fail-closed registry gate, including both unchanged mandatory Phase 2.1 security tests, which now fail during fixture setup. The pre-gate baseline had two failures at their intended assertions. See [the V3 handoff](docs/handoff/FINAL_REPORT.md).

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

## Registry-independent development commands

```sh
cargo run --locked -p rit-cli -- repo inspect /path/to/existing/repo --json
cargo run --locked -p rit-cli -- repo worktrees /path/to/existing/repo --json
cargo run --locked -p rit-cli -- dependencies inspect --repo /path/to/existing/repo --json
cargo run --locked -p rit-cli -- activity reconcile --repo /path/to/existing/repo --json
```

`rit checkpoint create --repo PATH --commit-only` writes a Git ref for committed HEAD **only**; it does not capture staged, unstaged, untracked, ignored, submodule or LFS contents. Full capture and restore are not implemented. `rit mcp serve --repo PATH` exposes a local opt-in read-only scope, not authenticated authorization or filesystem isolation. Activity is polling, not a notification watcher. Git 2.46+ is required conservatively; only Git 2.47.3/Linux was tested. See [CLI reference](docs/CLI_REFERENCE.md) and [V3 handoff](docs/handoff/FINAL_REPORT.md).
