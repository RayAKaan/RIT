# RIT V4 implementation and handoff

**Verdict: materially advanced development preview; incomplete and not production ready.** The Phase 2.1 security gate has not passed and is not an accepted risk. Writable registry and legacy adoption remain fail-closed. See `V4_IMPLEMENTATION_AUDIT.md` for design/evidence and `deferred-work.yaml` for stable blocker IDs; older V3 reports are historical. No remote push, PR, CI, native macOS/Windows execution, or independent security review is claimed.

## Capability matrix

| Area | Implemented and locally exercised | Remaining gate or limitation |
|---|---|---|
| Managed workspaces | RIT creates real linked Git worktrees with opaque IDs, separate branches and CAS Git manifest refs; list/inspect/status, lock/unlock, reconciliation, interrupted/orphan detection; five RIT-created worktrees tested | Git metadata is unauthenticated; same-user races/crash handling require review. Unsafe trees/filters use explicit no-checkout. Removal, even for clean worktrees, remains disabled; no force deletion. A worktree is not an OS sandbox |
| Git subprocess safety | Linux x86-64 child-process restriction for status/diff and mutating worktree/merge operations; hook/filter/merge-driver adversarial fixtures | Narrow Linux-specific mitigation, not an independently reviewed sandbox; blocked filters can make inspection fail. Other platforms fail closed for restricted paths; Git >=2.46, Git 2.47.3 tested |
| Activity | `notify` native live events, bounded buffer, deduplication, overflow/restart and periodic Git reconciliation; five managed worktrees with concurrent edits exercised | Process-local, lossy on overflow or downtime; no authenticated actor, durable history or normalized stage/commit/branch events. Native Linux only |
| Dependency/impact | Bounded Tree-sitter source snapshots for commit/branch and non-atomic live worktree/workspace; partial Python, TypeScript and local Go import resolution, source hashes, evidence, cycles and reverse impact | Incomplete semantics, language/build-system coverage, symbol analysis, explicit file-set service and durable graph cache; `complete=false` and observations can stale immediately |
| Conflicts | Real temporary-object-store `git merge-tree` simulation with merge bases, changed paths, stage blobs/messages and advisory structural/binary/gitlink evidence; configured custom drivers refused | Semantic result incomplete; no Mergiraf, auto-resolution or application. Scratch result tree is ephemeral. No source/target ref update |
| Checkpoints | Git-object-backed bounded raw index plus staged entries, tracked worktree, explicitly included untracked and selected ignored files, symlink targets, executable bits, deletions and unmerged stages; list/inspect/fsck/tree consistency and Git GC survival tested | Non-atomic and always incomplete; 500 paths, 1 MiB/file, 8 MiB total; submodule data, LFS availability, split-index shared bytes and full filesystem metadata not guaranteed. Selected files may contain secrets. No restore or deletion; refs are unauthenticated |
| Submissions/candidates | Clean committed managed source submitted against observed local target; ephemeral dependency/checkpoint evidence; clean merge creates real detached worktree and two-parent candidate commit, reconciles staleness, runs limited Git fsck and partial static analysis | No production queue, authentication, approval, execution of repository tests/builds, protected-target update or publication. Conflicts produce evidence without a candidate. Interrupted creation can leave metadata/objects/directories |
| MCP/SDK | Ten opt-in scoped read-only local stdio tools, app-backed workspace/checkpoint/submission/candidate lists; Rust, Python and TypeScript interop; caller path arguments and mutation denied | Same-OS-user cooperative scope, not authenticated authorization or filesystem sandbox; no HTTP/privileged routes or full MCP conformance |
| Identity/audit/storage | Existing pure policy/identity/audit models and Phase 2.1 fail-closed gate | No production credentials, protected audit anchor, durable identity binding, safe hot-journal recovery, substitution protection, legacy adoption or independently reviewed Windows/macOS backends |
| Reliability/release | Bounded cross-phase fixture, local build/test logs, and targeted adversarial checks | Full production 40-step acceptance, independent security review, native macOS/Windows, remote CI and release approval not done |

## Cross-phase evidence

The `scoped_mcp_lists_real_managed_checkpoint_submission_and_candidate_records` test creates a workspace through RIT, observes a native event, analyzes live content, captures and verifies a content checkpoint, commits source, creates a submission with related checkpoint evidence, simulates conflict, constructs a real detached candidate with limited static/object checks, then reads the resulting records through a separate stdio MCP process. Separate tests exercise five actual RIT worktrees, concurrent watcher events, a conflicting submission with no candidate, target movement, missing candidate ref, original-index preservation, unmerged stages, deleted paths, oversized-file refusal and inconsistent checkpoint tree detection. This is a bounded fixture workflow, **not** the complete requested production acceptance scenario.

## Local validation

Environment: Linux x86-64, Git 2.47.3, Rust 1.99.1. Logs and exit codes are included in the separate V4 handoff package. No mandatory storage test was deleted, altered or ignored; both mandatory files are unchanged relative to V2 commit `0edcb69`.

| Command/check | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | exit 0 |
| `cargo test --locked --workspace --all-targets --no-fail-fast` | exit 101: **83 passed, 21 failed, 0 ignored** |
| Same test with `--all-features` | exit 101: **83 passed, 21 failed, 0 ignored** |
| Locked workspace all-feature debug and release builds | exit 0 each |
| Warning-denied all-feature workspace docs | exit 0 |
| Mandatory `marked_registry_recovers_hot_journal_after_killed_writer` individually | exit 101 at fixture `crash_recovery.rs:44`, `SecurityGateBlocked`; pre-gate intended recovery assertion failed `RecoveryBlocked` |
| Mandatory `refuses_replaced_marked_database_even_if_schema_and_application_id_match` individually | exit 101 at fixture `registry.rs:379`, `SecurityGateBlocked`; pre-gate intended assertion accepted substituted marked DB |
| Python stdio interop; TypeScript strict compilation and scoped stdio interop | exit 0 each |

The 21 suite failures comprise 8 registry-dependent CLI repository tests, 1 storage unit test, 1 crash-recovery acceptance and 11 registry tests. Both mandatory tests now stop at setup and therefore **do not prove the original vulnerabilities fixed**. The V4 first full run additionally found an outdated scaffold test expecting obsolete positional workspace syntax; it was corrected to check the current usage error without relaxing the other fail-closed assertions, and the final counts above reflect that correction.

## Persistence and boundaries

- Workspace manifests and branch refs, content checkpoint commits/refs, submission manifests and candidate refs/manifests persist in the Git repository across processes; these are mutable coordination data, **not authentication, protected audit or permission**. Workspace branches may be pushed by `git push --all`.
- Watcher events, simulations, dependency graphs, conflict reports and validation results are ephemeral; they can be lost or become stale. The content-checkpoint ref keeps captured blobs reachable, but a Git owner can rewrite or remove refs; verification checks consistency, not authorship or atomic completeness.
- RIT did not execute repository-supplied hooks, filters, custom merge drivers, builds or tests in its new workflows. A fixture invokes ordinary Git operations to prepare commits and a merge conflict; the candidate validation itself does not execute user code. Do not extend this to claim hardened isolation.
- Creation may leave discoverable `provisioning` metadata and/or unreachable objects or directories after interruption. Cleanup and safe restore are intentionally not enabled.

## Next work, preserving the gate

1. Independently design and validate exact-file registry identity, hot-journal recovery, protected state, secure adoption and SQLite-consistent backup; retain sidecars and unchanged mandatory tests. Validate Linux races and native Windows/macOS and obtain independent review.
2. Complete non-destructive workspace/candidate removal and recovery, race/fault tests, cross-platform process restrictions and independently reviewed OS execution isolation. Do not represent a Git worktree as a sandbox.
3. Finish loss-aware watcher lifecycle and Git transition events, language/symbol/build-system analysis, safe complete checkpoint/restore and recovery, comprehensive conflict categories, authenticated integration queue/CAS publication, and full approval/audit controls.
4. Add protocol conformance and authenticated authorization, native release tests and the full 40-step production workflow; keep privileged operations disabled until requirements pass.

## Revision history

Starting V3 HEAD: `a6444622e41de50c8b69f338a1d410e747cbaf0c`. V4 commits: `8141b76` baseline audit, `3964e3c` filter mitigation, `3689d8e` managed workspaces, `7092f13` native watcher, `bc1cdb3` revision/worktree dependency analysis, `61c5098` isolated conflict simulation, `d91e751` bounded content checkpoints, `b5f2d44` non-publishing candidates, `55b6ea3` scoped MCP views, `ac1b254` cross-phase and failure-path tests. The final handoff documentation commit follows these; `git rev-parse HEAD` gives the exact tip. Original phase branches, including the Phase 2.1 checkpoint `c84004ebd62512345630801a7c219904468eacbc`, remain intact.
