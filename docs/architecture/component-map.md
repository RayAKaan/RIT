# Component map

## Implemented

- `rit-core::config`: `Config`, validated `LogLevel`/`LogFormat`, `ConfigFile`, `Overrides`, `resolve` and file status. No environment globals are read by the core.
- `rit-core::error`: `ConfigError` with preserved I/O/TOML causes.
- `rit-core::version`: Cargo package version.
- `rit-cli::cli`: command parsing; `main`: discovery, precedence, logging startup and exits; `output`: diagnostic output and logging configuration.
- Integration tests execute the compiled CLI, isolated from the invoking user's configuration.

## Planned, not implemented

Repository manager/Git adapter, SQLite storage/migrations, identity/policy, workspace/sandbox providers, event/reconciliation, awareness, checkpoints, transactional integration, conflict/dependency adapters, daemon, agent coordination, local API, MCP and SDKs. Introduce crates only when functionality and dependencies justify them. All interfaces must share application services and policy, not duplicate business logic.
