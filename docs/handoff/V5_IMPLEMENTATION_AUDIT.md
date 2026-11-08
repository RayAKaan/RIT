# V5 starting-state audit and implementation traceability

## Verified baseline

Original repository `phase/integrated-v4` was clean at `f41f9323fe3942c4a433faf7ec972c4f0bc16169`; V3 `a6444622e41de50c8b69f338a1d410e747cbaf0c`, Phase 2.1 checkpoint `c84004ebd62512345630801a7c219904468eacbc`, and other phase branches remain. V5 starts on `phase/integrated-v5`. The mandatory storage integration test files are unchanged from V2 `0edcb69`; `Registry::open` and `adopt_legacy` remain fail-closed. See V3/V4 audits and blocker records for original hot-journal and database-substitution evidence.

Fresh `cargo test --locked --workspace --all-targets --no-fail-fast` on V4 exited 101: **83 passed, 21 failed, 0 ignored**. Classification: 8 registry-dependent repository CLI tests, 1 storage connection unit test, 1 hot-journal acceptance and 11 registry integration tests; the library gate prevents fixture setup or replaces the expected storage-specific error. The mandatory tests therefore do not reach the original security assertions; the gate is not a fix. Full output is in the V5 baseline log.

## V4 capability map and V5 priorities

| Capability | Actual implementation and entry point | Existing evidence | Limitations and V5 work |
|---|---|---|---|
| Git reads | `rit-git/src/process.rs` and `GitReadService`; `rit repo` | `rit-git/tests/git.rs`, CLI tests | Linux x86-64 seccomp child restriction for sensitive commands; no hardened sandbox or native platform acceptance |
| Workspaces | `Git::workspace_create`, `workspace_reconcile`, `workspace_lock`; `WorkspaceService`; `rit workspace` | `creates_and_reconciles_five_actual_rit_workspaces`, filtered no-checkout test | Unauthenticated CAS manifest refs; no safe remove/move or automatic crash recovery; retain no-checkout fallback, enhance operation intent/reconciliation without deleting uncertain data |
| Activity | `rit-application/src/watcher.rs`, `activity.rs`; `rit activity watch` | overflow/restart and five-worktree native-event tests | Buffered ephemeral native events can be lost; native Linux test only; Git transitions not normalized; add explicit reconciliation evidence |
| Dependency/impact | `rit-analysis`, `Git::source_snapshot_*`, `GitReadService`; `rit dependencies`/`impact` | revision/Go/workspace tests | Partial resolution and non-atomic live snapshot, no full staged overlay or symbols; extend conservatively |
| Conflict | `Git::simulate_merge`, `ConflictService`; `rit conflicts simulate` | isolated merge stages, structural evidence, malicious-driver test | Temporary object store and advisory evidence, no automatic resolution/Mergiraf; broaden evidence while preserving refs |
| Checkpoints | `Git::content_checkpoint_create/list/verify`, `GitCheckpointService`; `rit checkpoint` | staged/raw/untracked/ignored/symlink/mode, deletion/unmerged/size refusal, forged tree, GC and index preservation tests | Non-atomic, incomplete, fixed limits, submodule/LFS/split-index limitations, no restore. Prioritize exclusive non-destructive new-destination recovery, integrity verification and operation evidence; do not overwrite existing work or claim complete capture |
| Submission/candidate | `Git::submission_*`, `candidate_*`, `SubmissionService`; `rit submission` | clean detached candidate, stale target, conflict and deleted-ref tests | Unauthenticated metadata, no durable review/approval, limited interruption recovery, no cleanup/publication; improve reconciliation without deleting data |
| MCP | `rit-protocol/src/lib.rs`, `ReadOnlyScope`; stdio | real app-backed cross-phase records and Rust/Python/TS clients | Ten no-argument scoped tools, same-user not authentication; no mutation, HTTP or full conformance |
| Identity/audit | `rit-core/src/security.rs` pure logic | unit tests | No authenticated principal, protected audit or production credentials; blocked storage boundary unchanged |
| Cross-phase workflow | `scoped_mcp_lists_real_managed_checkpoint_submission_and_candidate_records` | real RIT workspace, notify, analysis, checkpoint, candidate and stdio reads | Not V5 30-step acceptance, no restore/removal/interruption recovery proof |

## Persistence architecture and trust

Commit history and blobs live in Git objects; branches in Git refs. Workspace manifests and worktree metadata, checkpoint Git objects and refs, submission manifests and candidate commits/refs/worktrees are durable coordination metadata but mutable and unauthenticated. Conflict simulation uses temporary objects and ephemeral reports. Dependency graphs are recomputed and tied to source snapshots; watcher events are in-memory and lossy. Identity/credentials and protected audit have no production persistence until storage/security prerequisites are independently validated. Git repositories, refs, file ownership and worktrees are not authenticated actor identity, protected audit, an execution boundary, or a filesystem sandbox. Do not place credentials in them.
