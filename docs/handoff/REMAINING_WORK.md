> **V5 update:** See [V5_FINAL_REPORT.md](V5_FINAL_REPORT.md) and [V5_IMPLEMENTATION_AUDIT.md](V5_IMPLEMENTATION_AUDIT.md) for current capabilities, actual 30-step diagnostic results, validation and open blockers. Earlier descriptions below are historical, not a superseding V5 status.

> **V4 update:** Current capabilities, validation and limitations are in [V4_FINAL_REPORT.md](V4_FINAL_REPORT.md). Older statements below are historical; stable blocker IDs remain open.

> **V3 status:** This document includes historical V2 statements; current capabilities, gate failures, validation and limitations are in [FINAL_REPORT.md](FINAL_REPORT.md). Stable blockers are updated in `deferred-work.yaml`.

# Remaining work

See `deferred-work.yaml` for stable blocker IDs, evidence, dependencies, interim behavior, remediation and verification. Priority order: storage identity/recovery and native backends; safe adoption/backup and independent review; authenticated actors, credential issuance/revocation and audit anchoring; central enforcement for old and new operations; validated workspace OS isolation and safe cleanup; Git-reconciled activity; complete checkpoints; integration queue/CAS/recovery; human-reviewed conflict and evidence-bounded impact tooling; authenticated API/SDK; collaboration/reliability; native release verification. The checked-in model types are neither providers nor end-to-end implementations.

## V4-specific remainder

The V4 partial implementations are itemized in `V4_FINAL_REPORT.md`. Preserve the fail-closed Phase 2.1 APIs and mandatory tests. Complete non-destructive workspace/candidate removal and recovery; checkpoint atomicity, submodule/LFS/full metadata and gated restore; richer watcher transitions and loss recovery; comprehensive revision/symbol analysis; conflict semantics; authenticated submission/approval and protected CAS publication; native hardened isolation; authenticated audit, MCP conformance, native Windows/macOS validation and the 40-step production scenario. See new stable blockers RIT-V4-001 through RIT-V4-003.

## V5 prioritized remaining work

1. Independently resolve RIT-SEC-001/002/003 and pass unchanged mandatory tests and native adversarial review before production registry, identity, privileged MCP or publication.
2. Specify and implement safely fenced worktree removal/move and interruption recovery preserving ignored, untracked and concurrent data; keep disabled until demonstrated.
3. Design atomic or truthfully consistency-aware complete checkpoint capture with configurable resource/streaming strategy, durable restoration and tested crash windows, submodule/LFS payload retention, filesystem metadata and overwrite protection. Current subset recovery must not be represented as complete.
4. Complete authenticated submission lifecycle, candidate idempotent recovery/cleanup, policy and stale-target coordination; never mutate source or publish without gates.
5. Complete durable event loss reconciliation, semantic graph/conflict coverage, native macOS/Windows, independent security review and real remote CI. Rerun full 30-step V5 and production 40-step acceptance only when their prerequisites exist.
