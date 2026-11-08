# V3 starting-state implementation audit

## Verified baseline

Branch `phase/integrated-v2-final`, HEAD `0edcb69f1c8e36c5498ca398f517d3bf5c6941d4`, clean worktree. `phase/02-git-adapter` and `checkpoint/phase-02.1-deferred` both point to `c84004ebd62512345630801a7c219904468eacbc`; historical Phase 2.1 evidence is intact. See complete history using `git log --all --oneline --graph --decorate`. Origin is the independent RIT GitHub repository.

Exact fresh command: `cargo test --locked --workspace --all-targets --no-fail-fast`; exit **101**, **80 passed, 2 failed, 0 ignored, 0 measured, 0 filtered** (summing each test binary; includes helper test entries). `marked_registry_recovers_hot_journal_after_killed_writer` fails at `crates/rit-storage/tests/crash_recovery.rs:119`: `hot-journal recovery should succeed without adopting an unrelated database: RecoveryBlocked`. `refuses_replaced_marked_database_even_if_schema_and_application_id_match` fails at `crates/rit-storage/tests/registry.rs:395`: `a substituted database with matching SQLite marker and schema must not be authorized`. The reported 78/2 count was an earlier V2 run; two subsequent passing tests, `legacy_adoption_cli_fails_closed_without_creating_storage` and `doctor_never_calls_schema_readability_security_health`, explain the later 80/2 count. Both tests and all baseline failure evidence remain unchanged.

## Existing implementation at baseline

- `rit-git/src/process.rs`: real read-only Git discovery, porcelain status, refs and bounded HEAD history, subprocess timeout and output limits; `rit-core/src/git_object.rs` validates full algorithm-aware object IDs. Not all requested CLI commands use application services.
- `rit-application/src/lib.rs`: real repository registration and live lookup via experimental SQLite; path inspection; no worktree/checkpoint/integration provider. Existing registry operations are unsafe for valuable data and unauthenticated.
- `rit-analysis/src/lib.rs`: bounded Tree-sitter static import-syntax extraction from caller-supplied source, not repository graph extraction or module resolution.
- `rit-core/src/activity.rs`: pure snapshot comparison and exact-path overlap. No live watcher or durable events.
- `rit-core/src/{workspace,checkpoint,submission,conflict,security}.rs`: models and limited pure logic, not production worktree, checkpoint, integration, authenticated policy, or semantic services.
- `rit-protocol`: local discovery prototype, empty tool list, every tool call denied; Rust/Python/TypeScript clients test discovery only.
- `rit-storage`: public `adopt_legacy` remains callable despite the CLI guard; `Registry::open` can write through a substituted marked DB and is untrusted. Both require fail-closed production library behavior; disabling them may prevent old acceptance fixtures from reaching their original failure point. Do not weaken, skip or falsify tests, and record both baseline and post-gating results separately.

## Previous artifact inspection

The original commit `0b6b5bb` contains two generated Python `.pyc` files with machine-specific paths; commit `e878e31` removes them. Original history was not rewritten. `RIT-handoff.bundle` verifies and advertises all previous 20 local branch names and a HEAD; it contains separately sanitized history with changed hashes. `RIT-history-map.txt` maps original to sanitized commits. `RIT-source.zip` has 108 files, passes CRC, and contains no `.pyc` or the specified local paths. These observations do not constitute a complete secret audit.

## V3 safety boundaries

Do not claim Phase 2.1 passed. Do not delete journals, open unknown SQLite databases for writing, assume worktrees are security sandboxes, execute arbitrary repository-controlled scripts as a substitute for isolation, or publish protected refs. Read-only observations must preserve sensitive data and limits. Every exposed operation must be explicit about authentication, persistence, concurrency, crash and platform limitations.

## V3 mandatory library fail-closed change

V3 gates both `Registry::open` and `adopt_legacy` before any filesystem or SQLite access, with no bypass flag or test-only provider. This intentionally makes existing writable-registry acceptance fixtures fail earlier: they can no longer create the setup registry through the disabled production API. In particular, the mandatory recovery and substitution tests remain untouched and fail during fixture setup rather than reaching the baseline assertions above. This is **not** a passing substitution or recovery test and does not remediate either defect; the baseline evidence above remains the last full reproduction. Restoring genuine fixture coverage requires a separately reviewed safe test fixture strategy, not reopening the production API or pretending these failures pass.
