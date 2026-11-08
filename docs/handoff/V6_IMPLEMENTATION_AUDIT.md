# V6 implementation audit

## Actual paths and evidence

| Area | Entry points and real behavior | Boundary / incomplete work |
|---|---|---|
| Workspace | CLI `workspace` → application workspace service → `rit-git` intents, worktree identity and refs; dry-run removal reports blockers | Owner claims are unauthenticated; no safe destructive remove, move, or same-user fencing |
| Checkpoints | `checkpoint create/verify/restore/extract/extractions` → Git content objects/ref CAS → filesystem; tests preserve actual source/index and inject SIGKILL/collision | Schema 3, bounded buffered I/O; no global snapshot, full metadata, submodule contents/LFS payload, automatic resume, in-place restore or retention GC; symlink exports can point outward |
| Integration | submission/candidate CLI → application → real detached merge commits and intent state; stale target/fsck failures return errors | No sandboxed build/test execution, verified identity, authorized approval, atomic multi-ref state or publication |
| Conflicts | real Git merge-tree plus separate changed-path and revision-bound Tree-sitter import-edge evidence | Advisory only; semantic behavior, rename/dynamic import/Mergiraf coverage incomplete; first merge base and bounded graphs |
| Activity | watcher notification channel + Git endpoint reconciliation, restart resets generation and reports loss | No complete history, authenticated actor, durable event store or portable native validation |
| Impact | revision/worktree and staged-index Graph via `rit-analysis`; index uses stage-0 Git blobs, rechecks listing/HEAD | Concurrent changes/reversions can escape detection; unmerged entries fail closed; parser/import/test reachability heuristics incomplete |
| MCP/SDK | `rit-protocol` scoped stdio delegating to actual application services, Python and TS clients | Read-only, local OS-user trust; no identity, full conformance or path confidentiality against that user |
| Collaboration | model types, operation reconciliation and inspectable records | No persistent task/queue, real approval, authenticated activity/audit or automatic global recovery |
| Storage | `Registry::open`/adoption fail closed before writable operation | No trusted DB identity or safe hot-journal recovery; production security plane remains gated |

`V6_IMPLEMENTATION_MATRIX.md` records baseline and concrete tests/commits. The watcher diagnosis is plausible but not proven as the single baseline failure cause. Generic Git/FS/SQLite changes cannot be globally atomic; persisted intent allows inspection, not automatic transactional recovery. Ref and manifest CAS protects cooperative races, not adversarial authentication. Omitted metadata and boundaries must be presented to users, never silently represented as complete capture or authorization.

Review of CI: all-feature no-fail-fast tests, all-feature build/release/docs, and Linux Python stdio are defined; mandatory security tests run independently even on failure. Remote CI execution and native outcomes are unknown. Local TypeScript interop is separately tested. No bypasses were added to storage gate or original mandatory test files.
