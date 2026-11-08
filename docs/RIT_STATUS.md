# RIT status — Phase 2.1 storage gate

**Status: BLOCKED; Phase 3 prohibited.** Starting-state verification for the current task: branch `phase/02-git-adapter`, HEAD `e92c1d4eb12d91329cd55f1014c8feed534e1f64`; worktree was clean; ancestors `8d050b3`, `c7de701`, `619b5a3`, `c6a1041`. The independent RIT origin was absent from the restored local Git configuration and was re-established as `https://github.com/RayAKaan/RIT.git` without changing history. Remote phase-1 push dry run exited 128 because GitHub authentication was unavailable.

The storage crate uses `rusqlite 0.41.1` / `libsqlite3-sys 0.39.1` with bundled SQLite **3.54.1** (source ID `2026-10-16 14:45:20 555e07ef33f69a0957d24d8e555b702464a131d6f1607f4c3c1e4551e8e6ca76`). Application connections use SQLite's default VFS: no custom VFS is passed. The registry normally uses the default rollback journal, `synchronous=FULL`, five-second busy timeout and enabled foreign keys. Its experimental Linux-only `openat` traversal, mode/owner/link checks, lock, SQLite no-follow and v1 application marker do not yet establish the independently bound file identity required for safe writable recovery. Non-Linux registry operations fail closed; real macOS/Windows backends do not exist.

`Registry::open` and `adopt_legacy` first use read-only SQLite validation before a read-write connection; `open_readonly`, `diagnose` and `verify` also use read-only SQLite. A truly hot rollback journal returns `SQLITE_READONLY_ROLLBACK`; current code refuses recovery. Existing explicit legacy adoption has no durable adoption intent or SQLite-consistent pre-migration backup and does not meet the new adoption protocol. The hot-journal acceptance test at the starting commit was `#[ignore]`, and its explicit run was previously reported to fail; the current task must make it a required test and report its actual result.

No Phase 3 identity or security implementation exists. Native macOS/Windows execution and remote matrix results are not known. See `RIT_DECISIONS.md`, `RIT_RISK_REGISTER.md`, and `phases/phase-02-remediation-report.md` for current design, evidence and unresolved issues. Never delete a hot SQLite journal or silently adopt a database. Preserve a database and its associated journal together.

## Current follow-up results

The required real hot-journal test is no longer ignored and fails `RecoveryBlocked`; a required marked-database substitution test also fails because the wrong database is accepted. The ordinary required test command exits 101. A protected-manifest state machine is documented but **not implemented or approved**, and `(device,inode)` reuse was reproduced on local ext4. There are still no macOS/Windows storage backends, safe legacy-adoption backup/intent protocol or independent security review. No Phase 3 branch exists. See §17 of the Phase 2.1 report for exact test evidence, CI changes and the complete blocked checklist.

## Owner-authorized structural continuation

The owner explicitly superseded the prior prohibition on Phase 3+ *scaffolding* while preserving the Phase 2.1 security gate. New persistence-dependent identity, audit, credential and privileged operations must fail closed. This does not mark Phase 2.1 or any later phase accepted. See `RIT_ROADMAP.md` and `handoff/deferred-work.yaml` for live statuses.
