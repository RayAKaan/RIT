# RIT V6 final report

**Status: substantive development milestone; overall acceptance BLOCKED. Not production-ready.** Branch `phase/integrated-v6` preserves and descends from original V5 `d1ae9edda88d2211b343f4f6cf18d0201aa105ef`; no original history was rewritten. See `V6_IMPLEMENTATION_MATRIX.md`, `V6_IMPLEMENTATION_AUDIT.md`, `V6_SECURITY_REPORT.md`, `V6_TEST_RESULTS.md`, `V6_LOCAL_HANDOFF.md`, and `deferred-work.yaml` for details and reproduction.

## Delivered, bounded behavior

- Workspace removal preview inspects actual worktrees, lock state, submissions, checkpoints and bounded ignored paths. It is **not an authorization**; all actual removal remains disabled. Concurrent RIT creation is exercised with one winner.
- Checkpoint schema 3 records configurable bounded capture policy and time; legacy schema 2 verifies/restores with defaults. Up to 2,000 paths, 4 MiB/file and 32 MiB aggregate within other index/manifest limits; defaults unchanged. Captures are incomplete and non-atomic. Selective extraction exports captured worktree bytes to an absent directory with CAS intent/pin and interruption inspection. It is not an in-place or staged-index restore and does not automatically clean up/resume.
- Candidate validation now refuses stale/uninspectable target and real Git fsck failure; partial creation returns the recoverable ID. Candidate construction uses actual Git, but source and refs are unauthenticated, worktrees are not sandboxes, arbitrary validation execution and protected publication are unavailable.
- Conflict output includes actual merge-tree results and advisory cross-file changed-importer/dependency evidence; staged index dependency and heuristic impact analysis use actual index blobs and detect observed changes on recheck. Both remain bounded, partial and non-atomic; findings are not semantic proof or complete test coverage.
- Watch restart uses a fresh bounded channel and reports a gap. Notifications remain lossy/process-local and lack protected durable history. The one baseline watcher failure did not reproduce in eight isolated pre-fix runs, so its cause is not established.
- Fifteen opt-in scoped read-only MCP tools use real services; real Python and TypeScript stdio interop passes locally. There is no authenticated authorization, sandbox, privileged MCP or full protocol-conformance claim.
- A real 35-position disposable diagnostic exercises Git workspaces, checkpoints, extraction, submissions, candidates, conflicts, interrupt/reconciliation and scoped MCP. It reports **15 PASS, 11 PASS_PARTIAL, 9 BLOCKED**, `full_acceptance=false`, exit 1. Registration, principal/agent identity, permissions, persistent tasks, executed validation, protected approval/publication and audit are blocked. Ordinary Git operations in the fixture prepare/verify work; managed entities are created via RIT.

## Release decision

Both default and all-feature locked full suites have **103 passed, 21 failed, 0 ignored**, exit 101. The two unchanged mandatory Phase 2.1 tests fail fixture setup with `SecurityGateBlocked`; their security assertions did not pass. Historical ungated investigation also showed failed journal recovery and accepted marked DB substitution. Do not bypass the gate. Formatting, all-feature warning-denied Clippy, debug/release builds, warning-denied docs, and scoped Python/TypeScript interop passed locally. No native Windows/macOS, independent security review, remote CI, remote push or release claim is made. The tiny benchmark is descriptive only.

The separate sanitized export is a transformed history, not the original checkout or sole source of truth; IDs change. Review the delivery verification document and history map before use. No force-push or production credential use.
