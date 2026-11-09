# ADR 0002: Git inspection and repository registry

Status: proposed Phase 2 design; only implemented portions should be treated as available.

## Context

Phase 0 requires Git-owned history, local-first coordination, controlled Git plumbing and separate SQLite metadata. Phase 1 supplied a core and CLI but no Git or persistence. Worktrees share a common Git directory, and filenames may not be UTF-8.

## Decisions

1. Introduce `rit-git` for typed read-only Git operations, `rit-storage` for SQLite, and `rit-application` for orchestration; all depend on core types, never on each other except composition in application. CLI is the composition root. No Git or SQLite belongs in core.
2. Use native Git through `std::process::Command`, argument vectors, bounded concurrently drained output, deadline and process-group termination. Strip inherited `GIT_*`, disable system/global config for these read-only queries, disable optional locks and interactive prompting; suppress raw stderr because it can contain secrets. Do not assert this is a sandbox for malicious repositories.
3. Probe porcelain v2, `--show-object-format` and `--is-shallow-repository`; Git 2.29 is proposed minimum because object-format inspection is required. Reject unsupported/unknown versions/capabilities rather than infer compatibility. Test installed Git 2.47.3; other versions/platforms remain unverified.
4. Use Git `rev-parse` for per-worktree gitdir and common dir and canonicalize the common dir as the deduplication key. Linked worktrees share the repository ID and have separate locations. Bare paths are locations, not working trees. Repository and location paths must be representable as Unicode in SQLite and JSON; reject non-Unicode without lossy conversion. Remote names only, no URL storage or printing.
5. Choose random UUID v4 for opaque typed repository IDs: mature OS randomness, no timestamp leakage, no path derivation; unlike UUID v7 it does not sort by creation time. `uuid` features v4/serde, no custom crypto. IDs remain stable on duplicate registrations.
6. Use `rusqlite` with bundled SQLite for consistent cross-platform builds; MIT rusqlite, public-domain SQLite according to project documentation. Maintain one local `rit.sqlite3` under selected data directory. Default XDG data on Linux, Application Support on macOS, LocalAppData on Windows. Explicit CLI path outranks environment; do not fall back to cwd. Schema-1 Phase 1 config remains unchanged.
7. Initial SQLite schema (version 1): `repositories(id TEXT PRIMARY KEY, common_git_dir TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, object_format TEXT NOT NULL, is_bare INTEGER NOT NULL CHECK(is_bare IN (0,1)), created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)`; `repository_locations(id INTEGER PRIMARY KEY, repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE, path TEXT NOT NULL UNIQUE, git_dir TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('worktree','bare')), first_seen_at INTEGER NOT NULL, last_seen_at INTEGER NOT NULL)`. A registered logical repository's `is_bare` describes its first observed location; each location is authoritative about its own kind. Timestamps are Unix seconds; no HEAD/status cache. Registration uses one immediate transaction and a unique common dir; duplicate location must belong to same repository, never be silently reassigned.
8. SQLite: enforce foreign keys, five-second busy timeout, default rollback journal, `synchronous=FULL`, local filesystem only, transactional `PRAGMA user_version` migration and reject future versions; no destructive auto-recovery. Use SQLite online backup/consistent backup process later; do not copy a live DB and assume consistency.
9. Status uses Git porcelain v2 with `-z`, preserving XY, submodule field, branch headers and both rename paths; unknown records or unsupported paths error rather than silently misinterpret. Untracked all is deliberate, timeout and byte limits bound work; ignored files excluded by Git defaults. Git-derived inspection is live, not persisted as history.

## Alternatives and revisit

Libraries replacing Git risk semantic divergence; avoid. WAL could improve reader/writer coexistence but adds shared-memory/local-filesystem constraints and is unnecessary for a small registry; benchmark before revisiting. UUID v7 leaks approximate creation time. Direct raw Git stderr or remote URLs may disclose credentials. OS-specific byte-path persistence can be added with a versioned encoding and migration. Full hostile-repository isolation and multi-host coordination belong to later phases.
