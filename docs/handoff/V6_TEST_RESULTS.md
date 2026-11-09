# V6 local test and performance results

Environment: Linux x86-64, Git 2.47.3, Rust/Cargo 1.99.1. Exact commands, output and exit codes are in accompanying `logs/summary.txt` and individual logs. No remote/native execution is implied.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS, exit 0 |
| locked all-target/all-feature Clippy `-D warnings` | PASS, exit 0 |
| locked default all-target no-fail-fast suite | FAIL, 103 passed / 21 failed / 0 ignored, exit 101 |
| locked all-feature all-target no-fail-fast suite | FAIL, 103 passed / 21 failed / 0 ignored, exit 101 |
| locked all-feature debug and release build | PASS, each exit 0 |
| all-feature docs `RUSTDOCFLAGS=-Dwarnings` | PASS, exit 0 |
| mandatory hot-journal test | FAIL at fixture setup `SecurityGateBlocked`, exit 101; underlying assertion not reached |
| mandatory database substitution test | FAIL at fixture setup `SecurityGateBlocked`, exit 101; underlying assertion not reached |
| Python real stdio interoperability | PASS, 2 tests |
| TypeScript strict compilation / Node scoped interop | PASS / PASS |
| 35-step actual bounded diagnostic | INCOMPLETE: 15 PASS, 11 PASS_PARTIAL, 9 BLOCKED; false acceptance and exit 1 |
| tiny benchmark | PASS as harness execution only; not a performance acceptance |

All-feature failures: CLI repository integration 8, storage library 1, storage crash recovery 1, storage registry 11. These are failures, not ignored tests; the gate blocks expected setup or expected error assertions. Watcher tests passed in both final suites. A separate initial V6 baseline had 91 passed / 22 failed, including one restart failure; pre-fix and post-fix isolated repetitions each passed eight times, so cause is not established. Historical pre-gate failure logs are included for diagnosis. Mandatory test file comparison to V2 had zero diff. The successful full suites in no sense establish storage security.

## Performance smoke sample

Three serial samples, 3 workspaces, 12 tracked Python files totaling 253 source bytes, 2 vCPUs, Intel Xeon 2.60GHz, Git 2.47.3; subprocess startup/JSON included; cache uncontrolled/no flush. Python's SQLite 3.46.1 is recorded by the harness, **not** claimed as the RIT bundled SQLite runtime version. `benchmark.log` has all raw samples, Rust and OS info. Unavailable durable query, authorized execution, permission evaluation and publication were not benchmarked. No comparison or production latency claim is justified by this tiny fixture.

| Operation | Median ms | Min–max ms |
|---|---:|---:|
| `activity_reconcile_ms` | 311.601 | 261.889–363.912 |
| `candidate_create_ms` | 1614.625 | 1599.914–1641.829 |
| `checkpoint_create_ms` | 434.207 | 433.785–434.706 |
| `checkpoint_extract_ms` | 654.635 | 637.415–659.456 |
| `checkpoint_new_worktree_restore_ms` | 1004.735 | 981.217–1008.525 |
| `checkpoint_verify_ms` | 517.530 | 504.011–555.961 |
| `conflict_simulate_ms` | 102.348 | 101.646–104.644 |
| `dependencies_index_ms` | 573.506 | 570.335–573.992 |
| `dependencies_worktree_ms` | 308.969 | 300.917–322.955 |
| `mcp_stdio_start_and_inspect_ms` | 108.543 | 107.020–108.682 |
| `repository_inspect_ms` | 58.582 | 57.964–59.925 |
| `repository_status_ms` | 58.677 | 57.960–59.161 |
| `workspace_create_ms` | 362.829 | 361.671–364.308 |
| `workspace_reconcile_ms` | 537.614 | 534.581–538.844 |
