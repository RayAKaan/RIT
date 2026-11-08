# Report A — Phase 2.1 storage remediation

**Date:** 2026-11-08. **Verdict: NOT COMPLETE. Phase 3 gate: FAIL.**

The Linux mitigations below are experimental; they do not establish a completed cross-platform security boundary. A genuine hot-journal recovery test fails. No Phase 3 branch, identity tables, credentials, or Phase 4 work were created. This report supersedes neither the historical Phase 2 report nor the prior Phase 3 stop report; it records subsequent work.

## 1. Initial repository state

Preflight found clean branch `phase/02-git-adapter`, HEAD `8d050b3af48438480adc0ebe6f8a70df84e25b5f`, based on Phase 2 `c7de701e059cca3b0cdab9bec51029c2b2702b79` and Phase 1 `619b5a3fb44e97088a203a4b4b0420d53dec06c3`; origin was `https://github.com/RayAKaan/RIT.git`. No reset, force push or default-branch modification was performed. The earlier Phase 2, Phase 3 stop, architecture, code, tests and CI were inspected.

## 2. Storage vulnerabilities

The previous report demonstrated that a database symlink could cause an unrelated SQLite database to acquire RIT tables and that hard-link aliases could make two apparent registries share writes. Pre-open path and link checks did not bind SQLite's later filename open to a validated file; another user able to modify an ancestor or data directory could replace entries. The old database had `user_version=1` but no RIT identity marker, so unrelated databases were ambiguous.

A separate crash-recovery defect was **reproduced during this remediation**, not assumed: after a writer transaction spilled dirty SQLite pages and was killed, an actual hot rollback journal remained. The read-only identity preflight could not recover it; list and registration failed. A correct fix must protect unrelated databases *and* allow safe recovery and correct SQLite locking. The current code reports `RecoveryBlocked` and does not delete the journal. **Preserve the database and journal together; do not remove the journal.**

## 3. Threat model

`docs/security/storage-threat-model.md` records assets, protected other-UID adversaries, untrusted filesystem locations and concurrent RIT processes, and the assumptions about local filesystem semantics. Same-UID malicious processes, root/administrators, kernel/filesystem compromise and broken SQLite locking or fsync are explicitly outside the application boundary. RIT authorization would not be OS isolation.

## 4. Architecture and trust argument under review

The proposed simpler alternative to a custom SQLite VFS is a private owner-controlled directory with validated ancestry and protected DB/sidecar entries. SQLite continues opening by pathname; the trust argument is that another UID cannot change a validated path, not that a directory handle magically pins SQLite's file. Canonicalization alone is not treated as a defense. `SQLITE_OPEN_NOFOLLOW` remains supplementary; `SQLITE_OPEN_EXCLUSIVE` is not used as SQLite exclusive creation. ADR `docs/architecture/decisions/0003-storage-boundary.md` is **provisional and not accepted**. Linux implementation needs additional review, filesystem/ACL/mount assumption analysis, adversarial tests and recovery work before acceptance.

## 5. Platform implementation

Linux: `rustix` handle-relative `openat`/`mkdirat`, `O_NOFOLLOW|O_DIRECTORY`, `fstat` checks for root- or effective-user-owned ancestors, rejection of group/other-writable non-sticky ancestors, effective-user-owned final directory with no group/other access, and mode-0700 creation. Trusted-owner sticky ancestors are allowed under Linux sticky-directory semantics. The final directory handle is retained while the registry connection exists. New database files and initialization lock files are created at 0600; existing DB, journal/WAL/SHM and lock entries are checked for regular type, owner, single link and owner-only permissions. Existing insecure paths are rejected rather than silently repaired. This cannot protect against same-UID replacement and has not been security-audited.

macOS and Windows: an appropriate safe owner/DACL/ACL, ancestry and reparse-point implementation **does not exist**. Registry opens intentionally fail closed on those platforms, including normal registry-dependent CLI operations. This is a known compatibility regression in this unfinished branch, not successful cross-platform remediation. Windows Rust cross-check and Clippy compile the fail-closed code; no native platform execution was performed. macOS cross-check failed because the host lacks an Apple-compatible C toolchain/SDK. A target being unavailable for native execution is not itself the security verdict; the missing boundary and hot-journal recovery are actual implementation blockers.

## 6. Database identity and adoption

New databases use SQLite `application_id = 0x52495431` (ASCII `RIT1`) and schema version 1 in the initialization transaction. Ordinary opens require the marker and the exact two-table v1 SQL schema, no extra user schema objects, `quick_check`, foreign-key consistency and parseable repository IDs. Unknown IDs, future versions, corrupt or unexpected databases and existing empty unmarked files are rejected. An interrupted creation that leaves an unmarked empty file is refused rather than silently claimed; a safe recovery/adoption path for that case is unfinished.

`rit storage verify --json` is read-only. `rit storage adopt-legacy --confirm` requires explicit confirmation, an unmarked version-1 exact schema, integrity/FK checks, and a transactionally set application ID. The user is instructed to take a SQLite-consistent backup first. Logical schema resemblance is not proof of ownership; the confirmation is meaningful. Existing files that are not private are rejected; they are not chmodded automatically. During a disposable CLI smoke, the unmarked registry was refused by ordinary listing and by adoption without `--confirm`; explicit adoption succeeded, and the original repository ID and location remained present. This does not prove recovery across all crash points.

## 7. Links, files and sidecars

`SQLITE_OPEN_NOFOLLOW` and handle-relative no-follow directory traversal reject static symlinks on Linux. Database and named sidecar metadata are checked without following symlinks; single-link checks reject static hard-linked databases. The secure directory is intended to stop other users replacing validated entries during SQLite use, conditional on the trust assumptions. Automated tests cover static symlink and hard-link rejection and a symlinked journal; the directory protection is not proof against a same-UID attacker or an unsupported filesystem. Sidecar recovery is **not complete**. No arbitrary journal is deleted.

## 8. Directory replacement and repository overlap

The Linux directory path is traversed component by component and unsafe non-sticky writable ancestors are rejected; final directory permissions protect its database/sidecar names from other UIDs under standard local Linux permission semantics. A manual experiment as `nobody` failed to remove the DB, plant a journal symlink or rename the parent directory; the registry remained readable and had one record. It is evidence for that setup, not a proof for all ACLs, mounts or races. Automated adversarial concurrent parent/directory substitution testing is still missing. Registration uses component-aware containment checks in both directions for the inspected worktree, Git/common directories and data directory; tests cover both data-under-repository and repository-under-data. Same-user/path races and hostile repository replacement are not ruled out.

## 9. Concurrency and recovery

Initialization uses an owner-only `.rit-storage.lock` with a five-second nonblocking retry and exclusive-create of the new DB, followed by SQLite's immediate transaction. Eight simultaneous CLI registrations of the same real temporary repository produced one logical repository and one created response. The actual registry connection's FK pragma and foreign-key failure remain tested. Existing lock-timeout and rollback tests pass.

The process-kill test in `crates/rit-storage/tests/crash_recovery.rs` forces real dirty-page spill and checks the hot-journal magic before killing the writer. Its **acceptance test is ignored in the normal suite because it fails**. Explicit execution exited 101 with `RecoveryBlocked`. Different-repository cross-process registration, concurrent read/write, full process termination/recovery and adversarial path races remain unverified. Do not infer crash recovery from the passing default suite.

## 10. Phase 2 Git and listing tests

New real-Git fixtures cover behind=1/ahead=0, Unix worktree type-change `.T`, missing HEAD, inaccessible directory on an unprivileged Linux user, a valid `RIT_GIT_EXECUTABLE` override, old and unrecognized fake versions, and killing a non-escaped descendant process group. Parser checks were expanded for empty paths/headers, duplicate known headers and invalid status flags. A linked-worktree test proves a repurposed first registered location does not hide a later valid location. Availability annotations are available in the application listing representation. Existing tests for redirects, URL redaction, timeouts and output limits still pass. Comprehensive malformed porcelain, escaped descendant behavior and several platform-specific cases remain incomplete; the descendant test does not claim to stop processes escaping the group.

## 11. Test results and exact commands

Linux x86-64, Git 2.47.3, Rust/Cargo 1.99.1. After changes, environment `RUSTUP_HOME=/home/user/.cache/rit-rustup CARGO_HOME=/home/user/.cache/rit-cargo RUSTUP_TOOLCHAIN=stable PATH=/tmp/rit-cargo/bin:$PATH`:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | exit 0 |
| `cargo test --locked --workspace --all-targets` | exit 0: 55 test functions pass, including a no-op helper outside its subprocess invocation; **1 recovery acceptance test ignored** |
| `cargo build --locked --workspace` | exit 0 |
| `cargo build --locked --workspace --release` | exit 0 |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps` | exit 0 |
| `cargo test --locked -p rit-storage --test crash_recovery -- --ignored --nocapture` | **exit 101**, desired automatic recovery fails with `RecoveryBlocked` |
| `cargo check --locked --workspace --all-targets --target x86_64-pc-windows-gnu` | exit 0 after installing MinGW; compiles the intentionally fail-closed non-Linux storage stub, not a Windows security implementation |
| `cargo clippy --locked --workspace --all-targets --all-features --target x86_64-pc-windows-gnu -- -D warnings` | exit 0; no native Windows test |
| `cargo check --locked --workspace --all-targets --target x86_64-apple-darwin` | exit 101: host C compiler rejects Apple `-arch` and deployment-target options; no Apple SDK/toolchain |

Rust target installation first failed with `No space left on device` on the 992 MiB `/tmp` mount; copying rustup into excluded workspace cache and retrying succeeded. The first Windows cross-check failed due to missing MinGW; installing `gcc-mingw-w64-x86-64` allowed the reported retry to pass. Dependencies and system packages in caches/VM may not persist in a workspace snapshot.

## 12. CI and remote state

The existing workflow includes Linux/macOS/Windows matrices, but this branch was not pushed and no remote matrix ran. Since registry functionality deliberately fails closed on non-Linux, existing native registry tests are expected to require a completed platform implementation; a successful cross-check is not a runtime pass. On the latest recheck, phase-1 push dry-run exited 128 because GitHub credentials were unavailable; remote `main` was `c6a1041fa9ddec01358f8dcf08319d209efc9107`, and the Actions API returned HTTP 200 with zero runs. These external blockers do not justify bypassing the local recovery/security gate.

## 13. Limitations and safety guidance

- Critical: hot-journal recovery is blocked by read-only identity validation. Preserve DB and journal; no automated repair is offered.
- Critical: macOS and Windows security implementations are absent; their registry access fails closed.
- Linux boundary, ACL/mount/filesystem assumptions and adversarial concurrent replacement require review; hard-link and sidecar checks are not a custom VFS or protection from same-UID processes.
- Unmarked v1 databases require explicit adoption after a consistent backup; exact schema comparison is deliberately conservative and can reject otherwise compatible variants. An unmarked empty file left by a crash requires safe manual review.
- Registration overlap preflight is not atomic with arbitrary same-user changes to the Git tree; no OS sandbox is claimed.
- Direct `rustix 1.1.6` Linux dependency uses already-locked packages; no complete dependency vulnerability/license audit was performed.

## 14. Remediation acceptance checklist

- [ ] Private per-user storage boundary **across supported platforms**: Linux experimental only; macOS/Windows missing.
- [ ] Unsafe data directory and parent replacement defense: Linux checks and limited test evidence exist; complete adversarial review/tests pending.
- [ ] No-follow, hard-link, file identity and sidecar protections: Linux partial; non-Linux absent; SQLite open is pathname-based.
- [x] RIT application marker and explicit conservative legacy adoption implemented and locally exercised; full crash-safe adoption/recovery not established.
- [x] Actual registry connection foreign-key enforcement tested.
- [x] Concurrent first-time initialization and same-repository registration exercised with eight processes.
- [ ] Different-repository concurrency, concurrent readers/writers and process interruption/recovery: incomplete; hot-journal test **fails**.
- [x] Stale-first-location lookup fallback and application listing annotations implemented with local real-Git test.
- [ ] Remaining Git/status, filesystem race and all-platform tests completed: not met.
- [ ] Phase 1/2 regression and CI gates: local Linux default suite passes with an explicitly ignored failing acceptance test; remote matrix and native platforms unverified.
- [ ] Security review completed: not met.

## 15. Verdict and next steps

**NOT COMPLETE. STOP before Phase 3.** Resolve identity-bound, concurrency-safe hot-journal recovery without mutating an unrelated database; design and implement the Windows DACL/reparse/handle and macOS ACL/ancestry boundaries; test cross-user and replacement attacks, hard links, sidecars, crash recovery, different-repository concurrency and the remaining Git edge cases; rerun the whole matrix and update this report. Do not create `phase/03-identity-security` or security tables until the local gate actually passes. The latest code remains Phase 2 work and is not a production security guarantee.

## 16. Verified starting state for the subsequent recovery directive

The next task started on `phase/02-git-adapter` at `e92c1d4eb12d91329cd55f1014c8feed534e1f64`, with a clean worktree and the reported four-commit ancestry. The restored workspace had no `.git/config` or origin; the specified independent RIT origin was re-added, and its Phase 1 push dry-run again failed exit 128 for missing authentication. Bundled `libsqlite3-sys 0.39.1` contains SQLite 3.54.1; rusqlite is 0.41.1. Connections use SQLite's default VFS. Linux is the only experimental backend and macOS/Windows fail closed. The recovery test was still explicitly ignored. See the living documents `docs/RIT_STATUS.md`, `docs/RIT_DECISIONS.md`, `docs/RIT_TEST_STRATEGY.md`, `docs/RIT_RISK_REGISTER.md`, and subsequent sections of this report for new evidence, not the earlier test results alone.

## 17. Recovery directive follow-up — actual results

**Verdict remains NOT COMPLETE; Phase 3 gate FAIL.** This section records work after the starting-state section. No production identity/recovery protocol or manifest was implemented, because the examined file-identity schemes do not yet establish the exact persistent file binding required before an unrelated file could be opened read-write. This is an unresolved security/data-integrity defect, not a GitHub authentication or native-machine availability stop. No Phase 3 branch or code was created.

### Root cause and evidence

`Registry::open`, `diagnose` and the current legacy adoption sequence perform read-only SQLite content validation before recovery. SQLite 3.54.1 returns `SQLITE_READONLY_ROLLBACK` when a hot journal must be replayed. A required real subprocess test now runs without `#[ignore]`: two records were committed, the separate writer used `journal_mode=delete` and `cache_size=10` to spill dirty pages, was killed with SIGKILL (signal 9), and left a 13,824-byte rollback journal beginning `d9 d5 05 f9 20 a1 63 d7` on tmpfs. Database and journal existed together. The ordinary registry reopen failed `RecoveryBlocked`, so the test **failed** before its subsequent rollback, integrity/FK, new-write and separate clean-restart assertions. These assertions are implemented but not reached and are not claimed to pass. The fixture does not yet prove an independent binding before writable open; that protocol is absent.

A second required test created two distinct clean RIT-marked databases, preserved the first, substituted a copy of the second, and called `Registry::open` on the first path. It returned success even though the database belonged to the second registry. This demonstrates that SQLite `application_id` plus exact schema does not independently identify the original file. The test fails by design until the defect is fixed. Both acceptance tests are now part of the normal test suite; neither is ignored. The code maps `SQLITE_READONLY_ROLLBACK` to `RecoveryBlocked` consistently in `storage verify` and the read-only legacy preflight as well.

### Design and identity investigation

`docs/RIT_DECISIONS.md` records the candidate protected-manifest approach, custom VFS tradeoffs, rejected shortcuts and an explicit proposed initialization/adoption/recovery state machine. It is **not approved or implemented**. One critical issue is stale OS IDs: on the `/home/user` ext4 filesystem a disposable file's `(device,inode)` pair was reused by the next created file after unlink. A manifest bound only to this pair can authorize an unrelated replacement. Windows documentation also says file IDs can be reused. SQLite warns against opening databases through multiple hard links with different journal names and against external database file descriptor close operations while SQLite holds POSIX locks. A same-user malicious process remains outside the application boundary, but accidental replacement is in scope.

An unprivileged Linux `name_to_handle_at` experiment returned opaque handles on tmpfs and ext4, and the two ext4 files with the same reused inode had different handle bytes. This is preliminary evidence only; platform guarantees, filesystem support, mount identity/reuse, privileges, persistence and potential generation reuse are not established. It was not used to authorize SQLite writes. Raw SQLite page/header inspection was not used for identity.

The current legacy-adoption command still lacks an independently bound durable adoption intent, a SQLite backup-API pre-migration backup, interruption recovery and verified backup. It does not meet the new directive. Do not treat it as safe for valuable data; take a SQLite-consistent backup first and do not delete a journal. No independent code review was available or claimed.

### Scope of inspection and platform status

`grep` review located all production SQLite connections in `crates/rit-storage/src/lib.rs`: read-only helper, legacy writable open, registry writable open, `open_readonly` and `diagnose`. Application/CLI reach them via storage services, although CLI repository listing opens storage directly. No explicit journal/WAL deletion, rename or overwrite was found in production code; SQLite itself manages its journals. This search is not an independent security review or proof of all indirect effects. The Linux-only backend remains experimental; macOS and Windows remain fail-closed stubs and their ACL/DACL/reparse/file identity implementations are not done. Native macOS/Windows execution and remote CI have not run. The existing CI matrix now includes explicit Linux recovery and substitution acceptance steps; both are expected to report failures until corrected.

### Commands and results for this follow-up

Environment: Linux x86-64, Git 2.47.3, Rust 1.99.1, Cargo 1.99.1, rusqlite 0.41.1, libsqlite3-sys 0.39.1, bundled SQLite 3.54.1/default VFS. Toolchain and Cargo cache had to be restored after the previous workspace snapshot. The shell used `RUSTUP_HOME=/home/user/.cache/rit-rustup CARGO_HOME=/home/user/.cache/rit-cargo PATH=/home/user/.cache/rit-cargo/bin:$PATH`.

| Command | Exit | Result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | pass |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | 0 | pass |
| `cargo test --locked --workspace --all-targets` | **101** | fails required hot-journal recovery test, zero ignored in that target |
| `cargo test --locked --workspace --all-targets --no-fail-fast` | **101** | 56 tests passed, 2 failed; both recovery and substitution failures visible, 0 ignored; the count includes two no-op subprocess helper entry points |
| `cargo build --locked --workspace` | 0 | pass |
| `cargo build --locked --workspace --release` | 0 | pass |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps` | 0 | pass |
| `cargo test --locked -p rit-storage --test crash_recovery marked_registry_recovers_hot_journal_after_killed_writer -- --nocapture` | **101** | hot-journal evidence above; `RecoveryBlocked` |
| `cargo test --locked -p rit-storage --test registry refuses_replaced_marked_database_even_if_schema_and_application_id_match -- --nocapture` | **101** | substituted marked database was accepted |

`cargo test --locked --workspace --all-targets` stops at the first failing test binary; `--no-fail-fast` was additionally run to expose both failures, not to conceal the required command's failure. `docs/RIT_TEST_STRATEGY.md` maps threats to controls and test status; `docs/RIT_RISK_REGISTER.md` records remaining critical and high risks. Platform cross-compilation was not rerun in this restored environment; historical results are in §11 and are not native validation.

### Phase 2.1 gate

- [ ] Independently and durably bind the exact DB file before writable recovery; the substitution test currently fails.
- [ ] Make real hot rollback recovery pass in the required suite and verify rollback, integrity/FKs, writes and restart; the recovery test currently fails.
- [ ] Implement and test every initialization, manifest, backup, adoption and recovery crash state; no such protocol exists yet.
- [ ] Implement real reviewed Linux/macOS/Windows security backends and associated ACL/DACL/reparse/replacement tests. Linux is experimental; macOS/Windows are absent.
- [ ] Establish secure explicit legacy adoption with consistent verified backup and interruption recovery.
- [ ] Complete threat-to-control adversarial tests, platform-native validation and independent security review.
- [ ] Re-run the full regression suite with no failing mandatory tests.

**Decision:** stop at this unresolved critical local security and recovery gate. Do not begin Phase 3 or later phases. Preserve the local branch; remote authentication is pending but is not the reason for the security stop.
