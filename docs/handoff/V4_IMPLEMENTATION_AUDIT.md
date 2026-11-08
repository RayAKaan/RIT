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

## Workspace design and traceability update

The implemented limited Linux x86-64 provider writes an unauthenticated JSON manifest blob under `refs/rit/workspaces/<uuid>/manifest`, initially `provisioning`, then `ready` or `no_checkout`, with compare-and-swap updates. It allocates `refs/heads/rit/workspaces/<uuid>` via expected-zero `update-ref` before `worktree add --no-checkout`; interruption leaves discoverable metadata and/or an orphan branch rather than cleaning up user data. `git push --all` can push these branches. Manifest claims must be reconciled against Git inventory and are not a principal or authorization record.

Materialization uses bounded `ls-tree` and raw `cat-file` content, `read-tree` into the new worktree's index, and exclusive no-follow descriptor-relative file creation, not checkout. It refuses symlinks, submodules, configured filters, checkout-affecting attributes, autocrlf, oversized/non-Unicode/unsafe paths and other unsupported content; these worktrees remain explicitly `no_checkout`. Hooks are disabled and Linux seccomp prevents child process creation for these Git operations; this is a narrow execution mitigation, not a sandbox. Parent directories must be owner-only with trusted non-symlink ancestry. No destructive workspace removal is exposed; Git's removal can discard ignored files and invokes another Git process. Same-OS-user concurrent interference, filesystem races, materialization semantics, permissions and crash windows still require independent review.

`crates/rit-git/src/process.rs` is the provider, `WorkspaceService` in `rit-application` is the application boundary, `rit workspace` is the CLI, and `creates_and_reconciles_five_actual_rit_workspaces` creates five worktrees through RIT before user-like fixture edits and commits, checks branch independence/lock/unlock and simulates moved/interrupted/orphan conditions. `filtered_workspace_is_real_but_explicitly_not_checked_out` verifies filter refusal. This is not the complete acceptance suite: removal and the broader lifecycle state machine remain disabled/incomplete.

The status/diff clean-filter regression is mitigated on Linux x86-64 using root-owned `setpriv` and a seccomp BPF child-process restriction; other platforms fail closed for these operations. Dirty submodules are inspected separately with the same restriction. This is not a general isolation provider or native platform validation; configured filters may make the Git operation fail rather than producing a diff. `worktree_hashing_never_executes_repository_clean_filter` reproduces the adversarial configuration and checks its marker remains absent.

## Native notification traceability

`rit-application/src/watcher.rs` uses the maintained `notify` recommended native watcher, disables symlink following, watches the actual Git inventory, attaches a workspace ID only when matched to a RIT manifest, normalizes file events, suppresses exact duplicates for 150 ms, uses a bounded nonblocking queue and reports overflow/rescan requests. Errors trigger restart; the CLI periodically refreshes watches and re-observes Git. `rit activity watch` defaults to notifications; an explicit `--count` retains V3 finite snapshot polling. Both JSON and human-formatted output are supported, with `--duration-ms` for bounded runs. No authenticated actor or durable historical event is invented; events missed during downtime or overflow are unrecoverable. Native rename pairing is only reported when supplied in one event; Git stage/commit/branch transitions are reflected in reconciliation snapshots, not yet normalized as dedicated events. Restart/drop cleanly releases the provider; OS termination remains process shutdown. Tested Linux only, no cross-platform acceptance claim.

`bounded_queue_reports_overflow_and_restart_reconciles` exercises real overflow and restart through the application provider; `native_watcher_observes_concurrent_changes_in_rit_created_workspaces` exercises five RIT-created worktrees, concurrent writes, native events, correct workspace correlation and periodic Git reconciliation via CLI. Neither proves complete notification history nor every OS event category.
