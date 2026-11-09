# Component map

## Implemented

- `rit-core::config`: `Config`, validated `LogLevel`/`LogFormat`, `ConfigFile`, `Overrides`, `resolve` and file status. No environment globals are read by the core.
- `rit-core::error`: `ConfigError` with preserved I/O/TOML causes.
- `rit-core::version`: Cargo package version.
- `rit-cli::cli`: command parsing; `main`: discovery, precedence, logging startup and exits; `output`: diagnostic output and logging configuration.
- Integration tests execute the compiled CLI, isolated from the invoking user's configuration.

## Planned, not implemented

Identity/policy, workspace/sandbox providers, event/reconciliation, awareness, checkpoints, transactional integration, conflict/dependency adapters, daemon, agent coordination, local API, MCP and SDKs. Introduce crates only when functionality and dependencies justify them. All interfaces must share application services and policy, not duplicate business logic.

## Phase 2 development branch

- `rit-core::repository`: typed UUID repository ID, inspection, status, registry and location types.
- `rit-git`: typed read-only process and porcelain status adapter.
- `rit-storage`: local SQLite registry and schema migration.
- `rit-application`: repository inspection/registration/lookup orchestration.
- `rit-cli`: composes adapters and displays results. No daemon or later-phase services exist.

These components are under development and are not Phase 2 accepted until the outstanding tests and CI described in the Phase 2 report pass.
