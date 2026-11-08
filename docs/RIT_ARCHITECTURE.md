# RIT architecture — current status

RIT is local-first coordination separate from Git history. Git remains the source-history authority; `rit-git` invokes Git for read-only inspection, `rit-storage` owns local registry persistence, `rit-application` coordinates workflows and `rit-cli` exposes them. Normal Git use is independent of RIT. The SQLite registry is not an OS sandbox or Git authorization boundary.

## Storage recovery — incomplete

The current storage implementation has a Linux-only experimental private-directory boundary with trusted-root traversal and supplementary SQLite no-follow flags. The default SQLite VFS is used; it is not bound to a caller-opened file handle. Directory privacy and an application ID are insufficient by themselves to establish independent ownership of the exact file before SQLite recovery. There is no completed manifest protocol. The recovery and replacement acceptance tests presently fail. Existing pre-remediation adoption needs a durable identity-bound intent and SQLite-consistent backup; they are not implemented.

macOS and Windows registry operations fail closed because real platform backends are absent. A compiling stub is not platform support. Do not store identity or credentials here. The owner subsequently authorized fail-closed structural work beyond Phase 2.1, not release or production use. See `RIT_DECISIONS.md`, `security/storage-threat-model.md`, and `RIT_RISK_REGISTER.md`.

## Structural additions (not production services)

`rit-core` defines pure security, workspace, observation, checkpoint, submission, conflict, impact and collaboration models; `rit-application::contracts` declares provider boundaries. Existing Phase 2 repository service remains unauthenticated. `rit-protocol` offers stdio discovery only and denies all tool calls. SDKs inject transport and do not supply authentication. There is no durable authority, sandbox, checkpoint restore or publication. The authenticated actor is deliberately non-constructible outside core pending a secure provider design. The audit hash chain needs an independently protected anchor to detect truncation or complete rewrite.

## V2 independent read-only additions

`rit-git` returns validated algorithm-aware Git object IDs for bounded ref and HEAD-history observations. `rit-analysis` uses pinned Tree-sitter grammars against supplied bytes and does not read files or authenticate clients; imports are syntax observations, not resolved graph edges. `rit-core::activity` computes exact-path overlap over supplied workspace snapshots. None is a substitute for a live authenticated service, complete impact analysis or isolation. `rit-protocol` exposes modern discovery subset plus legacy discovery, not privileged tools.
