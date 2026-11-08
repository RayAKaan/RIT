# RIT test strategy — storage gate

Run real Git repositories and SQLite databases in disposable directories; do not test by mutating the development repository or real credentials. The usual complete Linux checks are `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --workspace --all-targets`, debug/release locked builds and warning-denied rustdoc. A failing acceptance test must be part of the ordinary suite, not ignored or silently skipped. Explicitly run `cargo test --locked -p rit-storage --test crash_recovery marked_registry_recovers_hot_journal_after_killed_writer -- --nocapture` to capture recovery evidence. Also run `cargo test --locked -p rit-storage --test registry refuses_replaced_marked_database_even_if_schema_and_application_id_match -- --nocapture`. Both are currently expected to fail; do not interpret an unsuccessful command as a pass.

## Threat → required control → evidence

| Threat | Required control | Current evidence/status |
|---|---|---|
| Other user replaces DB or parent | Verified private directory and trusted ancestors throughout SQLite use | Linux partial; static tests and earlier `nobody` experiment only. Race/ACL/mount review pending. macOS/Windows implementation absent. |
| Different marked SQLite DB is substituted | Independent durable exact file identity bound before write/recovery | **FAIL**: `refuses_replaced_marked_database_even_if_schema_and_application_id_match`. |
| Process dies after dirty-page spill | Bound DB before SQLite read-write recovery; keep DB and hot journal, use default VFS recovery | **FAIL**: `marked_registry_recovers_hot_journal_after_killed_writer`, with real subprocess SIGKILL, journal magic/size and SQLite version/mode evidence. |
| Concurrent init and registration | Secure transition lock and SQLite locking/transactions | Same-repository 8-process test exists; different-repository, concurrent reads/writes, process death holding init lock and full recovery remain pending. |
| Unknown or unmarked data | No automatic adoption; explicit binding, SQLite-consistent pre-migration backup and recoverable intent | Existing schema/confirmation tests partial; durable intent, automatic consistent backup, crash safety and hot-journal adoption missing. |
| Symlink/hard-link/reparse/sidecar redirect | Platform-specific safe traversal and regular-owner-single-link checks | Linux static symlink/hard-link/journal tests; Windows reparse and macOS ACL tests not implemented. |
| DB inside Git source or metadata | Component-aware resolved overlap checks plus trust/race review | Static overlap tests exist; linked metadata and race coverage incomplete. |
| Schema corruption and foreign-key failure | SQLite integrity and FK checks on actual application connection | Real-connection FK test and quick-check exist; full crash/recovery FK and integrity acceptance assertions are present but not reached until recovery works. |
| Same UID/administrator/kernel attacker | Explicitly outside application-only permission boundary | Documented, not claimed protected. |

Required future tests include failure injection at every durable manifest/SQLite/backup transition; malformed and rolled-back manifests; exact file identity mismatch and ID reuse; genuinely different UID/SID path replacement; SQLite hot rollback and WAL as applicable; lock timeout/process death; native Linux/macOS/Windows security tests and CI. Repetition alone is not proof of race freedom. Never raw-copy an open SQLite DB or remove a hot journal to make a test pass.

## Later-phase foundations

Unit tests exercise pure state transitions, source-bound activity comparison, non-executable restore/integration previews, conflict evidence, graph traversal and stale revision labeling. `rit-protocol` has a real stdio subprocess denial test; the Python SDK has a stdio interoperability test. These do not prove storage, isolation, authorization or end-to-end safety. The earlier 26-step future scenario has been superseded by the v2 40-step scenario in `handoff/END_TO_END_PLAN.md`; it has not been executed.

## V2 tests and limits

The Git adapter has real SHA-1/SHA-256 ref/history and five-worktree overlap fixtures. Tree-sitter unit tests cover Python, TypeScript and Go static syntax, invalid UTF-8, size limits and parse errors; they do not prove semantic analysis. The modern MCP discovery subset has Rust stdio interoperability, and Python/TypeScript clients have discovery/denial subprocess checks. No authenticated product workflow or 40-step acceptance test was run. Use `PYTHONDONTWRITEBYTECODE=1 RIT_PROTOCOL_BINARY=<built-rit-protocol> python3 -m unittest discover -s sdk/python -p test_interop.py`; typecheck TS with TypeScript 5.7.3 then run `sdk/typescript/test_interop.cjs` with `RIT_TS_CLIENT` and `RIT_PROTOCOL_BINARY`.
