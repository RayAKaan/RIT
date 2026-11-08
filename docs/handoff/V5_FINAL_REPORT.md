# RIT V5 final handoff: bounded development preview

**Not production ready. Phase 2.1 remains blocked; the mandatory tests do not pass. The 30-step diagnostic workflow does not satisfy full acceptance.** See `V5_IMPLEMENTATION_AUDIT.md` for architecture, commands, evidence, risks and test names, and `deferred-work.yaml` for stable open blocker IDs. Older handoff statements are historical.

## Baseline and implemented progress

The original V4 base is `f41f9323fe3942c4a433faf7ec972c4f0bc16169`, and the Phase 2.1 checkpoint is `c84004ebd62512345630801a7c219904468eacbc`. V5 is on `phase/integrated-v5`, preserving original branches and history. V5 independently reproduced the V4 default baseline: 83 passed, 21 failed, 0 ignored; exit 101. Security gate and mandatory tests were not weakened.

- Workspaces: CAS-backed Git-ref provisioning intent, interrupted-state observation and strictly metadata-only repair for already verified matching inventory. No unsafe automatic deletion, cleanup, movement or removal.
- Checkpoints: selected content and raw index verification, consistency rechecks and restore of a captured subset exclusively into a *new* worktree with operation intent, no-follow/exclusive file writes and exclusive index installation. Original worktree never intentionally overwritten. Raw-index validation rejects malformed/mismatched or unsupported split indexes. Races and crash durability are not completely solved; there is no automatic restore resume.
- LFS/gitlinks: bounded safe local LFS pointer/payload evidence and enumerated gitlinks; neither LFS payloads nor nested submodule content are captured or recoverable. Local payload hash status is transient.
- Submission and candidate: persistent cooperative submission state, CAS cancellation, source/target reconciliation and candidate provisioning progress; detached candidate validation reruns object checks and partial static analysis, while builds and tests remain `NOT_RUN`. No automatic candidate recovery/cleanup, protected approval or publication. Cancellation and candidate creation are not atomic across refs.
- Activity and MCP: bounded real endpoint Git transitions, explicit watcher loss evidence and thirteen opt-in scoped read-only stdio tools. Transitions are not complete commands/actor history; local scope is not authenticated authorization.

Git refs and manifests are **unauthenticated coordination**, not principal identity or protected audit. Git worktrees are not security sandboxes. Filesystem and refs are not one atomic transaction. Checkpoints can contain secrets, omit content and metadata, and are always `complete=false`. Capture still has fixed path/per-file/total/index limits; configurable budgets and general streaming capture/recovery are not implemented. All supported mutating behavior is an experimental Linux x86-64 development preview; same-user races and other platforms require independent review.

## Actual V5 diagnostic workflow

Run `python3 tools/v5_bounded_workflow.py` after building the debug workspace. It uses a disposable repository with five RIT-created managed worktrees, five RIT-created checkpoints, RIT-created submission and candidate, actual watcher and conflicts, a killed candidate process, new-worktree subset restore, scoped MCP and Git integrity checks. Report: **18 PASS, 8 PASS_PARTIAL, 2 PARTIAL, 2 BLOCKED**, `full_acceptance:false`, process exit 0. Exit 0 means the diagnostic ran to completion, not that the acceptance criteria passed. Complete checkpoints, automatic candidate recovery, clean removal and dirty-specific removal refusal were not achieved. The production 40-step acceptance was not run.

## Local validation and remaining security failures

| Check | Result |
|---|---|
| Formatting; locked all-target/all-feature Clippy `-D warnings` | pass, exit 0 |
| Locked full default suite and all-feature full suite | each 92 passed / 21 failed / 0 ignored, exit 101 |
| Locked debug and release all-feature builds; all-feature docs with `RUSTDOCFLAGS=-Dwarnings` | pass, exit 0 |
| Python interop; TypeScript strict compile and scoped interop | pass, exit 0 |
| Original mandatory killed-writer recovery test | exit 101, fixture `Registry::open` fails `SecurityGateBlocked` at `crash_recovery.rs:44` |
| Original mandatory substituted-database test | exit 101, fixture `Registry::open` fails `SecurityGateBlocked` at `registry.rs:379` |

The 21 suite failures are the pre-existing fail-closed registry-gate setup failures in CLI repository, storage unit, crash-recovery and registry targets. They do not demonstrate that recovery or substitution is fixed. Before the gate, the original hot-journal security assertion failed `RecoveryBlocked` (`crash_recovery.rs:119`) and the substituted database was accepted (`registry.rs:395`); ext4 reused an inode. The two mandatory test files are unchanged from V2 `0edcb69`. Never delete a journal, silently adopt an unknown database or bypass the gate. Detailed original evidence and remediations remain under RIT-SEC-001/002/003 and related blockers.

No native Windows/macOS validation, independent security review, remote push, PR, remote CI, authenticated production identity or production acceptance is claimed. See `REMAINING_WORK.md` for priorities. The separately sanitized export maps rewritten IDs; never force-push it over the original branch history. Its selective secret scan is not a comprehensive security audit.
