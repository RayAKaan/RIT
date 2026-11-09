# V6 local handoff

## Source and reproducibility

Original repository branch: `phase/integrated-v6`, based on V5 `d1ae9edda88d2211b343f4f6cf18d0201aa105ef`. Inspect with `git status --short`, `git log --oneline --decorate -20`, and `git fsck --full`. Original history is retained locally. The independently sanitized bundle/archive is a distinct rewritten export, with mapped IDs and limited selective redaction; do not force-push it over the original. Follow the separate delivery verification and ref inventory. Read all V6 reports, matrix and deferred IDs before extending.

Tested environment: Linux x86-64, Git 2.47.3, Rust/Cargo 1.99.1. In the current workspace:

```sh
export RUSTUP_HOME=/home/user/.cache/rit-rustup CARGO_HOME=/home/user/.cache/rit-cargo
export PATH=/home/user/.cache/rit-cargo/bin:$PATH
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features --no-fail-fast
cargo build --locked --workspace --all-features
cargo build --locked --workspace --all-features --release
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --all-features --no-deps
cargo test --locked -p rit-storage --test crash_recovery marked_registry_recovers_hot_journal_after_killed_writer -- --nocapture
cargo test --locked -p rit-storage --test registry refuses_replaced_marked_database_even_if_schema_and_application_id_match -- --nocapture
RIT_PROTOCOL_BINARY="$PWD/target/debug/rit-protocol" PYTHONDONTWRITEBYTECODE=1 python3 sdk/python/test_interop.py
python3 tools/v6_bounded_workflow.py
python3 tools/v6_benchmark.py --samples 3 --files 12
```

The full tests and mandatory tests are expected to fail while the gate is active. Workflow exit 1 means incomplete acceptance, not infrastructure failure. Run TypeScript `tsc sdk/typescript/client.ts --target es2020 --module commonjs --strict --outDir <empty-temp-dir>` and `RIT_PROTOCOL_BINARY="$PWD/target/debug/rit-protocol" RIT_TS_CLIENT=<empty-temp-dir>/client.js node sdk/typescript/test_scoped_interop.cjs`; use a trusted locally installed TypeScript compiler. Logs are bundled separately; build outputs/caches are not included.

## Practical recovery and priorities

`workspace remove --dry-run` is inspection only; actual removal is blocked. Use `checkpoint extract ... --preview` then explicit `--yes --accept-incomplete` only to a new directory; inspect `checkpoint extractions` after interruption. Existing path, unknown data, or conflict means stop and inspect; do not delete automatically. Candidate partial errors identify the retained candidate; use reconciliation and inspect before any manual action. Preserve original workspace data and refs. Capture is a bounded subset, not backup.

First obtain reviewed trusted-storage design and native adversarial evidence (security report). Then enable authenticated identity/audit, authorized validation/approval/publication and protected privileged MCP only after dependencies pass. In parallel improve genuinely bounded snapshots, safe lifecycle/fencing, cross-file semantics, activity loss recovery, service-backed collaboration, platform-native backends and CI. Treat `deferred-work.yaml` as the blocker inventory; full acceptance is currently false.
