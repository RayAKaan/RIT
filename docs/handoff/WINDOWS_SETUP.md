> **V3 status:** This document includes historical V2 statements; current capabilities, gate failures, validation and limitations are in [FINAL_REPORT.md](FINAL_REPORT.md). Stable blockers are updated in `deferred-work.yaml`.

# Windows local-agent setup (not verified here)

Install Git for Windows and rustup using trusted distribution channels, and install the exact `rust-toolchain.toml` toolchain including rustfmt and clippy. In PowerShell, open a disposable checkout of the handoff source, run `git --version`, `rustc --version`, `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --workspace --all-targets --no-fail-fast`, `cargo build --locked --workspace` and `cargo build --locked --workspace --release`. Record actual OS build, filesystem, toolchain and results. Set `$env:RUSTDOCFLAGS='-D warnings'` before `cargo doc --locked --workspace --no-deps`.

The registry backend deliberately fails closed on Windows: build or cross-compilation does not validate DACLs, reparse points, file identity, process recovery or native security. Implement and test these on native Windows before claiming support. Do not use real repositories or credentials in fault tests. No Windows results are claimed in this handoff.
