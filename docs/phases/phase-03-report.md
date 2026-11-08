# Phase 3 report — historical stop, then structural continuation

> Sections 1–17 below record a prior stop decision and describe the state at that time. The owner later authorized fail-closed structural continuation; see the final section. The security gate remains blocked.

Date: 2026-11-08. This is a stop report, not an acceptance report. No Phase 3 branch, identity schema, authentication, permission enforcement or audit implementation was created. Phase 1 and Phase 2 remain unaccepted and unmerged. No Phase 4 work was performed.

## 1. Gate decision

**STOP.** The storage path currently has a security/data-integrity race that was not resolved to a defensible cross-platform standard. Static mitigations added here are not a race-proof solution. Do not build a security authority or store credentials on this foundation yet. The existence of passing tests is not acceptance of the transition gate.

## 2. Starting state and branch ordering

The starting branch was `phase/02-git-adapter`, commit `c7de701e059cca3b0cdab9bec51029c2b2702b79`, with Phase 1 `619b5a3fb44e97088a203a4b4b0420d53dec06c3` in its ancestry and a clean worktree. The partial gate fixes in this report remain on the Phase 2 branch, after that commit. They do not constitute a Phase 2 acceptance or a Phase 3 implementation.

## 3. Remote verification

On this date, `GIT_TERMINAL_PROMPT=0 git push --dry-run origin phase/01-foundation:refs/heads/phase/01-foundation` exited 128, requiring unavailable GitHub credentials. `git ls-remote --heads origin` returned only `main` at `c6a1041fa9ddec01358f8dcf08319d209efc9107`. GitHub's public pulls API returned HTTP 200 and zero PRs; the actions runs API returned HTTP 200 and zero workflow runs. The GitHub connector was unsupported. These observations are not proof of Linux, macOS or Windows CI passing; no push, PR or merge was made.

## 4. Reproduced symlink defect and mitigation

In a disposable directory, a `rit.sqlite3` symlink to an unrelated SQLite database with a `keep` row caused the unmodified CLI `init` to exit 0, set the unrelated database's `user_version` to 1, and add `repositories` and `repository_locations` tables. The original row survived in that experiment, but mutating another database is a data-integrity failure. The write and read opens now use `SQLITE_OPEN_NOFOLLOW`, and a regression test checks that the symlink target's data and schema version remain unchanged. This does not prevent every path replacement or hard-link attack. SQLite no-follow behavior on macOS and Windows has not been verified here; symlinked directory components can also affect portability.

## 5. Unrecognized database mitigation

A version-zero SQLite database containing unrelated schema is now rejected transactionally instead of acquiring RIT tables. The new regression checks preserved data and version. An empty unrelated SQLite file, or an unmarked version-one database whose structure happens to resemble RIT's, is not reliably distinguishable from a RIT database merely from `user_version`. The legacy v1 format has no application identity marker; any migration design must handle this ambiguity without silently claiming unrelated data.

## 6. Reproduced hard-link defect and partial mitigation

After the no-follow change, two disposable data directories were made to contain hard links to the same existing RIT database. Registering a second disposable Git repository through the second path succeeded and caused the first registry to contain two repositories. On Unix, a pre-open metadata check now rejects an already multiply linked database, with a regression test for that static case. The check occurs before SQLite opens by pathname; it is **not bound to the opened file identity**, and a directory entry can be changed between check and open. Non-Unix hard links are not checked by this mitigation. SQLite journal/sidecar naming and concurrent aliasing also need explicit review. No claim is made that the race itself was reproduced by this experiment.

## 7. Unresolved critical storage boundary

`Repositories::register` resolves/checks the future data directory against the Git directory and then `Registry::open` creates/opens paths later. The directory or database can be replaced in between. The storage implementation neither pins an approved directory and SQLite database identity through the open/journal lifecycle nor establishes and verifies a sufficiently trustworthy, restrictive parent-directory boundary across Linux, macOS and Windows. Checks on pathnames and hard-link counts are insufficient to prove identity at the point of use. In a directory writable by another principal, that principal can potentially redirect writes; even without an adversary, aliasing and concurrent relocation risk unintended database changes. This needs a reviewed platform-appropriate design, error handling, and adversarial/concurrency tests before relying on the database for security state. Same-user or privileged access to Git/database files is outside any application-only authorization boundary; fixing the storage issue would not turn RIT into OS isolation.

## 8. Foreign key enforcement

A new unit test uses the actual `Registry` connection: it checks `PRAGMA foreign_keys = 1` and verifies an invalid location insert fails with SQLite's foreign-key constraint error. This supplements the older test that enabled the pragma on a separate connection. It is not a v1-to-v2 migration test; that migration does not exist.

## 9. Stale path display

CLI repository listing now adds `location_availability` with a filesystem-only observation: `present_unverified`, `missing`, `not_directory`, or `inaccessible`. A removed-directory test covers the missing case. `present_unverified` does not establish repository identity or accessibility to Git. The plain `Repositories::list` method still returns persisted records without this annotation; the CLI applies the helper. Existing live-ID lookup still selects the first existing directory and checks the common Git directory afterward; it may report stale instead of trying another valid location. This remains unfinished.

## 10. Other Phase 2 gate work

Not completed or newly verified: real behind-greater-than-zero and type-change tests; missing HEAD and inaccessible-directory tests; comprehensive malformed-porcelain rejection; valid `RIT_GIT_EXECUTABLE` override; unsupported Git-version behavior; descendant termination evidence; concurrent registration; full path and symlink/race review. Existing timeout/output-limit, real Git, and other tests continue passing, but they do not substitute for these tests.

## 11. Identity and bootstrap

Not designed or implemented. There are no human or agent principals, principal lifecycle, owner bootstrap, or concurrency/crash recovery guarantees.

## 12. Authentication and credentials

Not designed or implemented. There is no credential issuance, credential file, rotation, expiry, revocation, hashing, comparison, or authenticated actor context. Do not use this branch as an authentication system.

## 13. Authorization and application boundaries

Not implemented. Existing repository commands and application services remain unauthenticated and unprotected. There are no roles, capabilities, scopes, explicit denials or audit-coupled permissions. No enforcement claim is made.

## 14. Schema and migrations

The database remains schema version 1; no v1-to-v2 migration was attempted. Existing repository/location data is preserved by the tested operations. Test coverage does not prove preservation under the unresolved races or all possible SQLite corruption scenarios.

## 15. Audit and threat model

No security-event log or hash chain exists. No Phase 3 threat-model ADR or architecture has been approved. A future hash chain alone cannot detect a privileged attacker rewriting an entire chain without an independently protected anchor.

## 16. Local validation and dependencies

On Linux, Rust `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Cargo 1.99.1, Git 2.47.3: the following all exited 0 after the partial fixes:

- `cargo test --locked --workspace --all-targets` — 40 tests passed.
- `cargo fmt --all -- --check`.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`.
- `cargo build --locked --workspace`.
- `cargo build --locked --workspace --release`.
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps`.

An earlier focused `cargo test -p rit-storage` also passed. Tests were run with `RUSTUP_HOME=/tmp/rit-rustup CARGO_HOME=/home/user/.cache/rit-cargo RUSTUP_TOOLCHAIN=stable PATH=/tmp/rit-cargo/bin:$PATH`. No Phase 3 CLI smoke exists. The only dependency manifest change is using the already-locked workspace `serde` in `rit-application`; no new third-party package was introduced. The previous Phase 2 report contains the dependency/license caveats; no new full transitive license or vulnerability audit was performed.

## 17. Platform status

Local Linux only. macOS and Windows were not tested. CI matrix runs were not observed. Static hard-link protection is Unix-only. The behavior of symlink/reparse-point handling and SQLite no-follow semantics on Windows remains unverified. Diagnostics and recommendations here should not be interpreted as platform security certifications.

## 18. Acceptance checklist and next steps

- [x] Verify local starting branch, phase ancestry, baseline checks, and remote gate status.
- [x] Reproduce and mitigate static database symlink redirection and static Unix hard-link aliasing; verify unrelated nonempty version-zero databases are not initialized.
- [x] Test actual application storage connection's foreign-key enforcement; expose coarse stale location status in CLI listing.
- [ ] Resolve and test database identity, directory replacement and SQLite sidecar races with a reviewed cross-platform design and a clearly stated trust boundary.
- [ ] Complete the remaining Phase 2 gate cases listed above; rerun all checks and update the Phase 2 report.
- [ ] Only then create Phase 3 branch and design and implement the requested schema, identity, credentials, authorization and audit, including the complete negative-path and crash-recovery suite.
- [ ] Obtain remote authentication, push branches in order, open PRs and verify actual Linux/macOS/Windows CI before any authorized merge.

Local fixes are partial, not a substitute for the unfinished critical gate. No completion is claimed.

## Structural continuation authorized after the stop report

The owner later authorized safe architecture scaffolding while explicitly deferring—not accepting—the Phase 2.1 gate. Current additions: typed principal IDs, lifecycle, pure default-deny role/scope evaluation, deterministic SHA-256 audit-chain functions, and provider interfaces; CLI identity/permission/policy/audit commands fail closed. None of these authenticates a caller, issues/stores credentials, persists security state or protects the existing Phase 2 repository services. The `ActorContext` has no public constructor; a reviewed provider design is still required. Hash-chain verification cannot detect a rewritten whole chain or truncated suffix without an independently protected anchor. Full identity, principal and credential lifecycle, escalation prevention, audit atomicity and migration remain blocked by `RIT-SEC-001` through `RIT-SEC-010`. Phase 3 is **scaffolding, not complete and not security-verified**.
