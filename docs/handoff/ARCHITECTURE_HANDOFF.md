> **V3 status:** This document includes historical V2 statements; current capabilities, gate failures, validation and limitations are in [FINAL_REPORT.md](FINAL_REPORT.md). Stable blockers are updated in `deferred-work.yaml`.

# Architecture handoff

Git remains the source of truth for source code; Rust crates form core models → adapter/provider interfaces → CLI, with an independent discovery-only stdio protocol crate. Read `../RIT_ARCHITECTURE.md`, `../RIT_DECISIONS.md`, `../security/storage-threat-model.md`, and the phase reports. Pure domain functions do not authenticate or authorize users. The Phase 2 repository CLI predates authentication and is experimental; new privileged commands return unavailable without opening the registry. No OS sandbox, secure credential repository, operational audit trail or cross-system atomicity is provided. Resolve the Phase 2.1 blockers before connecting persistent identity/authz or enabling privileged features. A worktree is not isolation.
