# V4 implementation audit and traceability

## Starting state

Verified original V3 branch `phase/integrated-v3`, HEAD `a6444622e41de50c8b69f338a1d410e747cbaf0c`, clean checkout; V2 historical baseline is `0edcb69f1c8e36c5498ca398f517d3bf5c6941d4`. Phase 2.1 branch and checkpoint both remain `c84004ebd62512345630801a7c219904468eacbc`. Remote is the independent RIT repository; no remote action verified. Mandatory test files have no changes relative to V2 baseline. V3 full-history sanitized bundle/map inspected; original history retains historical bytecode and path incident and must not be distributed without separate sanitization.

Fresh `cargo test --locked --workspace --all-targets --no-fail-fast` exited 101: **70 passed, 21 failed, 0 ignored**, confirming V3. Eight repository CLI tests depend on gated writable registration, one storage unit test, one crash-recovery acceptance and eleven storage registry tests likewise fail due to `SecurityGateBlocked`. These are not evidence the original two defects passed: pre-gate recovery failed `RecoveryBlocked` and substitution accepted a same-marker replacement, as recorded in `V3_IMPLEMENTATION_AUDIT.md`. No tests were ignored or weakened. Existing independent Git tests already bypass the registry; the eight old repository tests explicitly exercise registration and should not be rewritten into different behavior.

## Newly reproduced genuine regression

On a disposable Git repository, configure an external clean filter in `.git/config` and `.gitattributes`, modify a matching file, and invoke `rit repo diff PATH --json`. Git executes the configured clean-filter shell command despite `--no-ext-diff` and `--no-textconv`: these flags do not suppress clean filters during worktree hashing. This violates the intended V3 safety boundary. Status may also hash content in some cases. Fix or fail closed before exposing those operations; do not mistake a preflight-only check, which can race concurrent config changes, for an OS execution sandbox. Other Git subprocess behavior, hooks, attributes and partial clones must be evaluated before adding mutations.

## Initial requirement mapping

| V4 area | Starting code | Starting evidence / gap |
|---|---|---|
| Workspace lifecycle | `rit-core/src/workspace.rs`; disabled CLI | No RIT-created worktrees, durable Git identity, lifecycle, reconciliation or safe removal |
| Activity | `rit-application/src/activity.rs` | Polling current Git snapshots only, no native notifications or event history |
| Analysis | `rit-analysis/src/lib.rs`, `rit-git/src/process.rs` | Bounded committed-HEAD imports; Python/TS heuristic, Go unresolved; no working-tree/commit selection |
| Conflicts | `rit-core/src/conflict.rs` | Models, no safe simulation; Git merge drivers may execute commands |
| Checkpoints | `GitCheckpointService`, Git `refs/rit/checkpoints` | HEAD-only opt-in; no full content or restore |
| Integration | `rit-core/src/submission.rs` | Pure types only; no candidate or publication |
| MCP | `rit-protocol/src/lib.rs` | Six opt-in scoped read-only tools; real Rust/Python/TS client tests; no auth, HTTP or full conformance |
| Identity and security | `rit-core/src/security.rs`, `rit-storage/src/lib.rs` | Pure policy models; registry/adoption writes blocked; no durable authenticated security records |
| Validation | `docs/handoff/FINAL_REPORT.md` | Linux-only; missing native platforms, review and remote CI |

## Design constraint

Git refs and manifests are coordination metadata, never authentication. Git `worktree add` can run checkout hooks, clean/smudge filters, and configured commands; `merge-tree` can run custom drivers. Any implementation must either demonstrably suppress these paths without unsafe races or remain explicitly unavailable. A worktree is not an OS sandbox. Preserve data and never use force removal or unsafe registry bypass.
