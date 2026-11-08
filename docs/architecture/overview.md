# Architecture overview

RIT is a separate Rust product over normal Git repositories. Git owns commits, trees, blobs, refs, index, working trees and remote Git semantics. RIT will own local coordination metadata and policies; SQLite is provisional for a later phase and is not implemented. Git and metadata cannot be treated as one atomic transaction.

Today, `rit-core` provides deterministic configuration resolution, typed configuration errors and version information; `rit-cli` performs process startup, environment/file discovery, parsing, logging and presentation. Core does not initialize global logging. The CLI depends on core, never the reverse. No daemon, database, Git adapter or asynchronous runtime exists.

Future interfaces (CLI, MCP, API) must call shared application services. The future coordinator orchestrates independent domain services and reconciliation, rather than taking ownership of all domain logic. Controlled adapters will isolate Git, persistence, process execution and sandboxing. The Git executable is the initial intended integration surface; no Git object database will be reimplemented. See [dependency rules](dependency-rules.md) and [component map](component-map.md).

Local-first v1 is single-host coordination. Linked worktrees and separate clones are not security boundaries. Hardened execution requires independently verified OS controls. Publication requires validated candidates, compare-and-swap and crash reconciliation; protecting checked-out branches from unrestricted external Git requires stronger controls than a RIT lock. These are design commitments, not current features.
