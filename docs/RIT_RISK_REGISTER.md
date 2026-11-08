# RIT risk register

| ID | Severity | Risk | Current response / disposition |
|---|---|---|---|
| ST-01 | Critical | Hot rollback journal requires writable SQLite recovery before the read-only identity check can succeed. | Required real-process test fails `RecoveryBlocked`; preserve DB and journal, no Phase 3. |
| ST-02 | Critical | Different DB with same application marker and schema is accepted as the original registry. | Required substitution test fails. Independent durable file binding not implemented. |
| ST-03 | Critical | Device/inode or Windows file ID reuse may make a stale manifest identify a replacement file. | Actual ext4 same-device/inode reuse reproduced on the next file creation. Reject device/inode-only recovery design; platform identity proof unresolved. |
| ST-04 | Critical | No real macOS ACL/ancestry or Windows DACL/reparse/file-identity backend. | Registry access fails closed on non-Linux; implementation and native validation pending. |
| ST-05 | High | Legacy adoption lacks durable identity-bound intent, SQLite-consistent backup and interruption recovery. | Existing explicit confirmation/schema check is incomplete; do not claim safe adoption or use on valuable data. |
| ST-06 | High | Linux mode checks and pathname opening lack completed filesystem/ACL/mount and concurrent replacement review. | Experimental boundary only; do not claim universal defense. Same UID and privileged attackers remain outside model. |
| ST-07 | High | No independent review or exhaustive crash/fault injection. | Acceptance not met; require real external/independent review before approval. |
| ST-08 | High | Default suite previously masked the recovery failure with `#[ignore]`. | Removed ignore; normal test suite now fails transparently until corrected. |
| ST-09 | Medium | Git overlap checks and process-group termination are not race-proof or isolation. | Preserve Git independence; document limitations, add tests before phase completion. |

Do not delete SQLite journal/WAL files or copy a live database to work around ST-01. Restore/backup operations need SQLite-consistent procedures.
