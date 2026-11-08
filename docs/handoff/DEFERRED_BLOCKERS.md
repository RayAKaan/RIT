# Deferred blockers

Canonical structured records: `deferred-work.yaml`. Security gate is NOT PASSED; no blocker is an accepted risk. Existing Phase 2 CLI operations are not authenticated and the existing adoption path is incomplete.

| ID | Severity | Title | Evidence |
|---|---|---|---|
| RIT-SEC-001 | critical | Hot journal recovery | Real SIGKILL hot-journal test fails RecoveryBlocked |
| RIT-SEC-002 | critical | Database substitution and file identity | refuses_replaced_marked_database_even_if_schema_and_application_id_match fails; ext4 reused device/inode |
| RIT-SEC-003 | critical | Protected manifest/state machine | docs/RIT_DECISIONS.md describes only an unapproved proposal |
| RIT-SEC-004 | critical | macOS storage backend | Non-Linux storage fails closed |
| RIT-SEC-005 | critical | Windows storage backend | Non-Linux storage fails closed |
| RIT-SEC-006 | high | Legacy adoption | Existing adoption has no durable intent or automatic consistent backup |
| RIT-SEC-007 | high | Independent security review | No independent review performed |
| RIT-SEC-008 | high | Linux adversarial validation | Current handle/path boundary is experimental |
| RIT-OPS-001 | medium | Remote authentication | Phase 1 dry-run push failed with missing GitHub credentials |
| RIT-SEC-009 | critical | Production identity and authorization unavailable | Pure policy types exist; no production authenticator or service enforcement |
| RIT-SEC-010 | high | Existing Phase 2 repository operations are unauthenticated | rit-application Repositories and CLI registry operations predate Phase 3 |
