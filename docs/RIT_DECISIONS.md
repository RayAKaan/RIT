# RIT decisions — storage identity and recovery

**Status: design under review; no approval and no Phase 2.1 acceptance.** Written before recovery implementation for this task.

## The problem and the non-negotiable boundary

SQLite may need to write a database as soon as it reads a hot rollback journal. A read-only content-validation probe therefore cannot validate a legitimate hot database. Conversely, letting SQLite recover an arbitrary pre-existing database before independent identity authorization can mutate unrelated data. The application ID is a format marker and does not identify the underlying file. No code may delete, rename, truncate, manually replay or ignore journals. SQLite's ordinary VFS and locking must perform recovery.

## Alternatives evaluated

**A. Owner-protected manifest + OS file identity (candidate, NOT approved).** A strict versioned manifest, exclusively created and durably written in a verified private directory, can hold a storage UUID, state, fixed database name, application ID/schema and platform file identity. Access to the manifest depends on verified ownership, effective access policy, symlink/reparse rejection and non-replaceable ancestor entries, not on merely placing it next to the DB. The application lock serializes transitions; each durable manifest replacement requires a secure exclusive temporary file, file flush, atomic rename/replace and directory flush (or documented Windows equivalent). Unknown formats, inconsistent states, lost/malformed manifests and any mismatch fail closed without altering DB or sidecars. File identity must be obtained without reading/closing a raw SQLite database descriptor while same-process SQLite connections may exist. Normal recovery after binding must use SQLite's default VFS and locking, then check marker/schema/integrity before exposing data.

**Critical unresolved identity issue:** `(st_dev, st_ino)` is not a durable exact identity across deletion and recreation. In this VM, on the `/home/user` ext4 filesystem, a disposable file was deleted and the same device/inode was assigned to the very next newly created file. A stale manifest recording only this pair could authorize SQLite recovery against a replacement unrelated file. Windows documentation similarly warns file IDs may be reused. A UUID stored only in the manifest does not prove that the database is the same file. A UUID inside SQLite cannot be safely verified before writable recovery. Birth timestamps or extra header parsing have not been shown to provide the required identity guarantee. Do **not** implement automatic writable recovery on a device/inode-only manifest.

Research durable platform identities and their actual guarantees, filesystem support, replacement/reuse behavior and inability to bypass the OS access boundary; fail closed where exact identity is not provable. A Linux filesystem export handle might contain a generation number but is filesystem-specific; `name_to_handle_at` can be unsupported, `open_by_handle_at` can require privileges, and mount IDs can be reused. Their use has not been established as a complete solution. Filesystems, mount namespaces and file restore/copy behavior require explicit policy. On Windows, compare identity on verified open handles and examine the documented lifetime/reuse limitation; 128-bit IDs and volume IDs are not proof of uninterrupted object lifetime after deletion. A protected link anchor is not an automatic answer: SQLite warns against multiple database links and mismatched journal names. A backup, sentinel, manifest or UUID must not accidentally induce alternate SQLite database names.

**B. Capability-based SQLite VFS (not chosen).** A carefully implemented VFS could bind opens to OS capabilities, but must preserve SQLite locking, sidecar naming, rollback/WAL recovery, fsync and Windows semantics across all connection lifetimes. A custom VFS does not by itself prove crash durability or prevent misuse of aliases. It adds unsafe/platform-specific code and needs substantial review, fault-injection and concurrency tests. Do not build it on an unreviewed assumption that handles make it safe.

**Rejected:** canonicalize then pathname open; read-only preflight as an automatic recovery mechanism; read-write SQLite open before independent binding; treating an open directory fd, `NOFOLLOW`, SQLite application ID or manually parsed header as sufficient; `SQLITE_OPEN_EXCLUSIVE` as `sqlite3_open_v2` exclusive creation; manual journal deletion or recovery; silent legacy adoption.

## Proposed state machine — not implemented

All states require verified platform directory/ancestor security and a held application transition lock for transitions. A future manifest state is not authoritative unless its protected creation, atomic update and independent DB object binding are established. `fsync` and SQLite transaction durability must be applied at the indicated boundaries; errors and unknown state fail closed and preserve all files.

| State | Trusted facts and permitted operations | Interruption / next action |
|---|---|---|
| Absent | No DB or sidecar assumed to be RIT. Verify ancestors; create private directory with correct security at creation. | No DB access; if partial directory exists, revalidate from trusted root. |
| Private directory verified | Trust only its validated OS security and supported filesystem assumptions, not pre-existing entries. | If DB/sidecar already exists, enter explicit legacy/unknown path, not initialization. |
| Reservation durable | Create a versioned initialization reservation exclusively and flush file and directory. No DB authority yet. | If reservation malformed or mismatched, stop and preserve; don't claim arbitrary files. |
| DB exclusively created | Create new file exclusively at owner-only mode; establish platform identity without violating SQLite lock rules. | If crash precedes a durably bound identity, refuse automatic adoption; report exact paths and require safe operator review. |
| Identity bound durably | Persist reservation with provably exact file identity and storage UUID; flush and atomically publish. Only the bound DB and matching protected sidecars may be used by SQLite. | If manifest/object mismatch or identity proof unavailable, refuse any writable SQLite open. |
| SQLite initialization in progress | Open only the bound object through protected pathname, use standard VFS and SQLite transactions, set expected schema and application ID. | On restart, permit SQLite recovery only after re-establishing binding; verify SQLite state. Unmarked/partial schema must not be exposed. |
| Initialized DB, completion pending | Schema commit has finished, but durable complete manifest may not yet exist. | Revalidate exact binding, let SQLite recover, validate marker/schema/integrity, then complete transition; otherwise fail closed. |
| Valid initialized | Protected completed manifest, identity bound and SQLite marker/schema valid. SQLite owns database/journal locking. | Normal SQLite recovery is permitted only after binding; mismatched ID, schema or sidecars stops access. |
| Legacy awaiting confirmation | Pre-existing database has no RIT identity binding. Only safe directory/metadata checks and non-mutating inspection are allowed; display actual path and object identity. | No automatic SQLite writable open or adoption. Explicit owner decision required; preserve DB and sidecars. |
| Legacy intent durable | After confirmation, persist an intent bound to the exact authorized object and sync it before writable SQLite open. | If identity mismatch or incomplete intent, stop. A valid durable intent may resume only on the same proven object. |
| Legacy recovery / backup | After identity authorization, let SQLite recover using ordinary locking, validate exact schema, then make and verify a consistent SQLite backup before any migration. | Interrupted recovery must retain DB and journal; incomplete backup must not be accepted as complete or overwrite earlier data. |
| Legacy migration pending | After verified backup, migrate marker/schema transactionally; retain recoverable intent with backup identity. | Resume after identity check; reconcile SQLite marker/transaction and backup state, never create a fresh replacement. |
| Legacy complete | Validate recovered migrated database and backup, sync metadata, then atomically mark complete. | Expose registry only after completed state is durable and all checks pass. |
| Unknown / unsafe / substituted | No identity or security proof. | Fail closed with actionable diagnostics; preserve all objects; owner/maintainer must inspect backed-up evidence. |

Transitions are required to be tested after each write, sync, rename, SQLite commit, backup and crash point. Concurrent processes must see one serialized transition and continue to rely on SQLite locks for database contents. These are design requirements, not claims that the current code implements the protocol.

## Sources

- https://sqlite.org/c3ref/open.html
- https://sqlite.org/atomiccommit.html
- https://www.sqlite.org/howtocorrupt.html
- https://sqlite.org/c3ref/backup_finish.html
- https://man7.org/linux/man-pages/man2/name_to_handle_at.2.html
- https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information
- https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info

## Additional feasibility observation

On this VM, `name_to_handle_at` succeeded on both `/tmp` (tmpfs) and `/home/user` (ext4). In one disposable ext4 trial a replacement reused the original inode immediately but returned different opaque handle bytes. This is **only an observation**, not a documented guarantee across arbitrary filesystems, reboots, mount replacement, handle-generation wraparound or Windows/macOS. Linux manual pages state that not all filesystems support handles, that `open_by_handle_at` may require `CAP_DAC_READ_SEARCH`, and that mount IDs can be reused. No production protocol or writable SQLite authorization has been based on this experiment.


## V2 diagnostic and adoption change

`rit storage adopt-legacy` now returns unavailable instead of invoking the unsafe legacy adoption library API. The API itself remains exposed and experimental, so the security issue is not resolved. `doctor` now adds `overall_status` and `security_gate` to its schema-2 JSON and changes the marked, readable storage status from `ok` to `schema_readable_security_unverified`. Consumers matching the old status string must update. A successful doctor process exit means diagnostics were collected, **not** that the product passed security acceptance; the new overall field is `security_blocked` absent a runtime error. This is an intentional observable behavior change to prevent a false health claim.
