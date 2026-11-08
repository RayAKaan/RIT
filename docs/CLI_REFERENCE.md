> **V4 update:** Current capabilities, validation and limitations are in [V4_FINAL_REPORT.md](handoff/V4_FINAL_REPORT.md). Older statements below are historical; stable blocker IDs remain open.

# CLI reference — V3 development preview

Use `rit --help` and subcommand help. Global flags include `--data-dir` and `--git-executable`; the latter wins over `RIT_GIT_EXECUTABLE`. Git >=2.46 is conservatively required (only Linux Git 2.47.3 tested). Output supports `--json`; Unicode-only paths/status are currently supported. Exit 4 indicates runtime/unavailable.

Registry-independent commands: `rit repo inspect|status|branches|tags|log|diff|refs|worktrees|doctor [PATH] --json`; `rit dependencies inspect|refresh --repo PATH --json`; `rit impact --repo PATH --files PATH --json`; `rit activity reconcile|list|overlaps --repo PATH --json`; `rit activity watch --repo PATH --count N --interval-ms N --json`. Activity watch is finite polling of status snapshots, **not** an event watcher; it may miss changes. Dependency graph is partial and committed HEAD only.

On Linux, `rit checkpoint create --repo PATH --commit-only --json`, `checkpoint list --repo PATH --json`, and `checkpoint verify ID --repo PATH --json` write/list/check Git refs containing **committed HEAD only**. They do not capture staged, unstaged, untracked, ignored, submodule or LFS contents. Explicit `--commit-only` is required; full capture and all restore remain unavailable.

`rit mcp serve --repo PATH` explicitly serves scoped, unauthenticated same-user read-only tools over stdio; without a scope tools are disabled. It is not an OS sandbox. There is no HTTP or privileged MCP endpoint.

Writable registry APIs and `storage adopt-legacy` fail closed; registry-dependent commands cannot complete. Identity, credentials, managed workspace lifecycle, privileged agent execution, full checkpoint/restore, submissions/publication, and tasks remain unavailable. Doctor reports the blocked Phase 2.1 security gate. See `handoff/FINAL_REPORT.md` for exact test failures and caveats.
