# Setup

Install rustup and Rust 1.99.1 with rustfmt and clippy. From this checkout run `cargo build --locked --workspace`. The toolchain file requests this exact toolchain. Network access is needed for initial toolchain/crate downloads; local tests themselves need no remote service. On Linux the tested architecture is x86-64; macOS and Windows are unverified until CI executes.
