# Phase 4 — workspace structure

Structural: typed workspace IDs, lifecycle models, requested isolation profiles and resource-limit metadata, provider traits and disabled workspace/agent CLI commands. Pure transition tests cover invalid transitions and terminal removal. Operational: no worktree creation, process execution, cleanup or durable storage. Security: worktrees are not sandboxes; hardened providers and authorization are absent. All new workspace/agent commands fail closed. Platform-specific OS isolation, crash recovery and safe cleanup are deferred; no claim of Phase 4 completion.
