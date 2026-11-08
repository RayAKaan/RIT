# RIT architecture — current status

RIT is local-first coordination separate from Git history. Git remains the source-history authority; `rit-git` invokes Git for read-only inspection, `rit-storage` owns local registry persistence, `rit-application` coordinates workflows and `rit-cli` exposes them. Normal Git use is independent of RIT. The SQLite registry is not an OS sandbox or Git authorization boundary.

## Storage recovery — incomplete

The current storage implementation has a Linux-only experimental private-directory boundary with trusted-root traversal and supplementary SQLite no-follow flags. The default SQLite VFS is used; it is not bound to a caller-opened file handle. Directory privacy and an application ID are insufficient by themselves to establish independent ownership of the exact file before SQLite recovery. There is no completed manifest protocol. The recovery and replacement acceptance tests presently fail. Existing pre-remediation adoption needs a durable identity-bound intent and SQLite-consistent backup; they are not implemented.

macOS and Windows registry operations fail closed because real platform backends are absent. A compiling stub is not platform support. Do not store identity or credentials here and do not start Phase 3. See `RIT_DECISIONS.md`, `security/storage-threat-model.md`, and `RIT_RISK_REGISTER.md`.
