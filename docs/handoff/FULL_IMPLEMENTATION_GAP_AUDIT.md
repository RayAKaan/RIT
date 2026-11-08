# RIT v2 implementation gap audit

Audit baseline: local `phase/12-release-foundations`, `ebe84d4d0718306f4ae841f11a9cc2f945d7e42e`, clean worktree, with Phase 2.1 and all phase branches preserved. This is a requirement-to-code map, not a completion claim. See `deferred-work.yaml` for issue-level acceptance and dependencies and the historical phase reports for exact evidence. The original checkout remains authoritative. An existing exported bundle was a sanitized one-commit snapshot, not a phase-history bundle; the Phase 10 commit contains generated `.pyc` files with a machine-specific path. Its successor removes them. No history has been rewritten in the checkout.

| Spec | Code / evidence | Actual gap and dependency |
|---|---|---|
| §§4–7, 8 | `rit-core`, `rit-application`, `rit-git`, `rit-storage`, `rit-cli`, `rit-protocol` | Direction broadly maintained; canonical entity set, authenticated cross-entry request flow, operation protocol, event envelope, migrations and algorithm-aware OIDs incomplete. |
| §9 | `rit-git/src/{process,status}.rs`, `rit-application/src/lib.rs`, CLI repository tests | Real Git discovery/porcelain and experimental registry work. History/refs beyond bounded HEAD/ref inspection, full status category validation, relocation, unregister, repository-wide monitoring and all adversarial/process tests incomplete. Registry commands are unauthenticated and storage gate is failed. |
| §10 | `rit-storage`, security threat model and Phase 2.1 report §17 | Hot-journal and substitution acceptance fail. Identity binding, crash state machine, sidecar security, safe backup/adoption, Linux review, Windows/macOS native implementations and independent review absent. Do not delete journal or adopt unknown databases. |
| §11 | `rit-core/src/security.rs`, `rit-application/src/contracts.rs` | Pure principal/role/scope evaluation and hash-chain checking only. No credential provider, authenticated context construction, persistence, central service enforcement, secure bootstrap or external audit anchoring. |
| §12 | `rit-core/src/workspace.rs`, contracts, disabled CLI | Metadata transitions only. Real managed worktrees, tasks, concurrent isolation, process management, hardened OS sandbox and cleanup absent. A worktree is not a sandbox. |
| §13 | `rit-core/src/activity.rs` | Snapshot comparison and exact-path overlap with a five-worktree Git fixture. Watcher, Git collection service, real events, overlap detection, event reconciliation and persistence absent. |
| §14 | `rit-core/src/checkpoint.rs` | Caller-supplied metadata and never-executable preview only. Capture, integrity, retention, backup and actual restore absent. |
| §15 | `rit-core/src/submission.rs` | Non-executable target comparison only. Queue, Git candidate, validation, approvals, CAS publication and recovery absent. |
| §16 | `rit-core/src/conflict.rs` | Extracts unmerged Git status entries. No merge-tree, content-level classification, Tree-sitter/Mergiraf or semantic workflow. |
| §17 | `rit-core/src/impact.rs` | Computes reverse reachability over supplied edges with explicit incompleteness. Tree-sitter extracts raw static import syntax for three languages from supplied bytes; no import resolution, symbol graph or source repository analysis. |
| §18 | `rit-protocol`, `sdk/{python,typescript}` | Discovery-only stdio prototype supports legacy 2025-03-26 and a 2026-07-28 modern discovery subset, with empty tool list; no privileged tools, auth, shared services, HTTP API or full client SDKs. Rust, Python and TypeScript exercise real stdio discovery/denial. |
| §19 | `rit-core/src/collaboration.rs` | Pure task/review models only; collaboration, production reliability harness and benchmark measurements absent. |
| §20–21 | README, `docs`, `.github/workflows/ci.yml` | Partial docs, pinned Rust, one OS matrix. Missing required guides, release packaging, dependency/license review, actual native and remote CI evidence. Many specified commands unavailable or absent. |
| §§22–26 | storage threat model, contracts and disabled interfaces | No recoverable Git/SQLite operations or authenticated authorization pipeline. No validated agent isolation; privileged later operations disabled. Existing registry and the legacy adoption library API need security remediation; the legacy-adoption CLI is disabled. |
| §§27–32 | tests under each crate and historical results | Unit, real Git, CLI and security tests exist; two mandatory Phase 2.1 tests fail. Real multi-workspace/checkpoint/integration/end-to-end and native-platform security tests absent. Build success is not release acceptance. |
| §§33–40 | phase branches, handoff docs and previous artifacts | Earlier bundle was snapshot-only. A separately sanitized mirror retained branch names and commit parent topology; reachable objects, clone and branches were checked. Original checkout history must not be changed. |

## Implementation decision

Continue safe independent work without weakening acceptance tests. Protected production identity, credential, authorization, process execution, checkpoint restore and publication cannot be enabled before their prerequisites are verified. Existing Phase 2 registry and library adoption paths are experimental, the adoption CLI fails closed, and they do not satisfy the storage/identity contract. Read-only Git inspection does not authenticate the caller and is not a substitute for application authorization.

## Full end-to-end scenario

The 40-step scenario in the v2 mandate has **not** been run; it is listed in `END_TO_END_PLAN.md`. Most steps require services and providers that do not exist.
