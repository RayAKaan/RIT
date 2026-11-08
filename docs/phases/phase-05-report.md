# Phase 5 — activity awareness

Structural: source-bound Git-status observations, staleness checks and deterministic snapshot reconciliation, plus observer contract. The observation reuses Phase 2's read-only Git status types, distinguishing changes and changed HEAD; it cannot see intermediate events or keystrokes. Test exercises source snapshots and clock staleness. Operational: no watcher, event routing, background process, persistence, task mapping or new activity CLI. Watcher overflow/restart must force Git reconciliation. Authorization, secure storage and real integration remain deferred. This is not complete activity awareness.

## V2 continuation — exact-path overlap calculation

A bounded-by-input pure comparison now aggregates Git change snapshots from distinct workspace IDs under one repository, marks stale snapshots and separates Git XY index/worktree flags. It does not infer a merge conflict, match rename sources, normalize case or account for missed activity. A real disposable Git test creates five detached worktrees, makes independent overlapping changes, stages one and verifies five correctly reported snapshots plus preservation of the original worktree. RIT itself does not yet create those worktrees or run a watcher, so this does not meet the Phase 4/5 end-to-end acceptance gates.
