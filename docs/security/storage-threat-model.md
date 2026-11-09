# RIT local storage threat model — Phase 2.1 (work in progress)

**Status:** design target, not a claim that the current implementation meets it. The Phase 2 gate remains closed until implementations, adversarial tests and review substantiate it.

## Assets and scope

Repository registry identity and integrity, Git repository contents, SQLite database and recovery journals/sidecars, future security policy and credentials, and the confidentiality of future credentials. RIT is a local-first single-host application on a local filesystem that correctly implements SQLite locking, atomic operations and synchronization. Network/multi-host filesystems and an OS with broken locking are outside this model.

## Threat actors

* Another unprivileged local OS user is in scope. They may create entries in shared writable directories, pre-place symlinks, hard links and other objects, attempt path substitution, and run concurrent processes. They do not control the RIT owner's UID, trusted administrator-owned ancestors, the kernel, or filesystem permission enforcement.
* Accidental or concurrent ordinary RIT processes operating on the same approved storage location are in scope. Locks, uniqueness and transaction/recovery behavior must be verified separately from the attacker model.
* Untrusted user-supplied paths are in scope: a string path, canonicalization result, or ordinary existence check is not a trust boundary. Other users may own or control one or more path components or database/journal entries.
* Malicious processes running under the same unrestricted OS identity are outside this boundary. Owner-only permissions cannot protect the owner from themselves, and application authorization is not OS isolation. Ordinary accidental same-user races still warrant tests and fail-safe design, but pathname-based SQLite cannot be claimed immune to hostile same-UID replacement.
* Administrators, root, SYSTEM, kernel compromise, physical access, hostile filesystem drivers, compromised Git executable and broken hardware/fsync guarantees are out of scope. Git repositories remain independently accessible outside RIT.

## Intended boundaries and attacks

A database may only be used in a dedicated directory whose contents cannot be replaced by another local user. Every ancestor entry must be protected against replacement as well: validating only the final directory does not suffice. Unix designs should traverse from a trusted root using directory handles and no-follow operations, validate owner and access for every component, create the private directory with restrictive mode at creation, and account for sticky-directory rules and ACLs. Files, including SQLite sidecars, require appropriate ownership, regular-file/type and single-link checks. Windows must separately validate reparse points, owner, effective DACL, ancestry and file identities using appropriate handle-based APIs; Unix mode bits are not an equivalent. Unknown or unsafe configurations fail closed with actionable errors. A private final directory alone does not neutralize an attacker-controlled parent that can rename it.

SQLite opens by pathname; it does not adopt an existing general-purpose file descriptor through `sqlite3_open_v2`. A defensible pathname open therefore depends on proving that other local users cannot change the validated path or its contents throughout the connection's lifetime. Retain handles where useful, while being explicit that retaining a handle by itself does not pin the pathname. `SQLITE_OPEN_NOFOLLOW` is supplementary. `SQLITE_OPEN_EXCLUSIVE` is a VFS flag, not an exclusive-create flag for `sqlite3_open_v2`.

Hard-linked databases can cause different journal names for the same inode. The SQLite documentation warns that using aliases is undefined and can prevent crash recovery. Journal and WAL entries must be protected by the same directory boundary; they must never be removed or overwritten blindly because a hot journal can be needed for recovery. Ownership and permissions of a directory alone are not proof that a pre-existing file or sidecar is trustworthy. File identity, type, ownership, mode/DACL, link count and application identity must be checked. Errors must preserve unexpected objects and unrelated databases.

Database identity is not proven by a version number. New registries require a stable SQLite application ID and transactional initialization. Legacy version-1, application-id-zero data requires full conservative schema examination, explicit owner confirmation, and transactionally setting the ID; unknown schemas/IDs fail closed. Even an exact legacy schema can belong to another application, which is why confirmation is mandatory. Backup and recovery must not copy/delete live SQLite files without SQLite-consistent procedures.

Repository overlap checks must use component-aware, verified paths and prevent accidental creation under worktrees or Git directories. Checking before later opening is not a race-proof guarantee. Live filesystem observations are not persistent repository identity.

## Alternatives

1. Private owner-controlled storage and verified ancestors plus conservative file checks: preferred if the platform implementation actually enforces the boundary.
2. Capability/handle-based directory traversal: strengthens path validation but does not alone force SQLite to use a pinned database handle.
3. Custom SQLite VFS: potentially binds operations to capabilities, but substantially increases unsafe, portability, recovery and locking complexity and needs intensive review; do not assume it is safer without evidence.
4. Continue with canonicalize and a later ordinary SQLite path open: rejected as a complete defense.

## Current status and residual risks

The current Phase 2 branch has SQLite no-follow and static Unix hard-link checks, but does not yet enforce a private trusted directory/ancestor chain, protect all sidecars, mark database identity, or prevent replacement between validation and use. Windows ACL and reparse behavior has not been implemented or verified. These are current blockers, not accepted residual risks. No authentication, credential or authorization subsystem may be built on it until the gate is met.

## References

* https://sqlite.org/c3ref/open.html
* https://sqlite.org/c3ref/c_open_autoproxy.html
* https://www.sqlite.org/howtocorrupt.html
* https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-operations

## Phase 2.1 implementation progress (not accepted)

The Phase 2.1 branch now contains an experimental **Linux-only** traversal using `rustix` `openat`, `O_DIRECTORY`, `O_NOFOLLOW`, `fstat` and owner/mode checks. It accepts only root- or effective-user-owned ancestors and rejects non-sticky group/other-writable ancestors; the final directory must be effective-user-owned and owner-only. Missing directories are created with mode 0700. A root- or owner-controlled sticky directory may be used as an ancestor because other unprivileged users cannot rename a child owned by someone else under Linux's sticky rules. Database, SQLite sidecars and the initialization lock are checked for regular-file type, effective-user ownership, single link, and owner-only mode. The final directory handle stays open with the registry connection. New DB and lock files are created with exclusive/no-follow or no-follow operations at mode 0600; creation is serialized with an initialization lock. SQLite still opens by pathname, so this is a protected-ancestor trust argument, not a file-handle-bound SQLite VFS and not protection against the same UID. Mount configuration, extended ACL/filesystem assumptions and long-lived directory moves still need full security review and adversarial testing. Existing unsafe files are refused, not silently chmodded.

There is no equivalent macOS or Windows boundary yet: non-Linux registry access currently fails closed. The Linux implementation must not be called a cross-platform solution. Rust cross-compilation and native behavior are distinct.

New databases receive `application_id = 0x52495431` (`RIT1`) and schema 1 transactionally; ordinary opens require that marker and the exact schema. An unmarked v1 schema is inspected but adopted only through explicit confirmation. Existing pre-remediation databases are not deleted or silently adopted. A crash between exclusive empty-file creation and schema commit leaves an unmarked file which is refused rather than assumed to be RIT. Adoption requires a backup and secure directory/file permissions.

**Critical recovery blocker:** validating database identity with a read-only SQLite connection fails when a hot rollback journal must be recovered. A killed-writer fixture with actual dirty-page spill reproduces `SQLITE_READONLY_ROLLBACK`. RIT now reports that recovery is blocked and says to preserve the database and journal together. Do not delete the journal. Safely binding database identity, recovery, and concurrent SQLite locking while avoiding recovery of an unrelated database is not solved. Phase 3 must remain blocked.
