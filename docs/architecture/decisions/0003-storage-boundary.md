# ADR 0003 — Storage security boundary (provisional, NOT ACCEPTED)

## Problem

SQLite opens a filename rather than adopting a prevalidated database file descriptor. A symlink, hard link or replaced parent could cause SQLite to operate on another file, and aliases may use different rollback journals. The earlier Phase 2 pathname checks were not a sufficient security boundary.

## Design under investigation

Prefer a private per-user directory with verified, non-replaceable ancestors, plus no-follow and per-file checks, over a custom SQLite VFS. This approach relies on OS authorization to prevent another UID from changing path entries for the full connection lifetime; retaining the directory handle alone does not bind SQLite's pathname. Same-UID or privileged malicious mutation remains out of scope. Linux currently implements an experimental restrictive `openat` traversal. macOS and Windows fail closed; they need separate implementations including ACLs, reparse points and file identity. Refer to `docs/security/storage-threat-model.md`.

RIT application ID `0x52495431` marks new schema-1 databases transactionally. Pre-existing unmarked v1 databases must pass exact conservative schema checks and receive explicit confirmation through `rit storage adopt-legacy --confirm`; ordinary opens do not adopt. Recovery must not mutate an unrelated unmarked file.

## Unresolved decision and hard stop

Read-only preflight cannot validate an otherwise legitimate database when SQLite must recover its hot rollback journal. Reproduced with `crates/rit-storage/tests/crash_recovery.rs`; the acceptance test is explicitly ignored in the normal suite and fails when run. Recovery must be designed with a trustworthy identity binding and actual SQLite locking/concurrency behavior, not by removing the journal, trusting just a preflight path or blindly opening arbitrary databases read-write. Investigate a separately durable identity binding or an appropriately reviewed VFS and safe bootstrap/adoption/recovery protocol. Until it is resolved, no Phase 3 identity or credentials may be implemented.

## Consequences

Linux functionality is restricted to private, trusted directory trees; existing 0644 files and nonprivate directories are rejected with actionable errors until the owner arranges safe permissions. Non-Linux registry functionality is intentionally unavailable. All changes are pre-release and subject to review; no acceptance is claimed.
