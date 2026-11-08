# ADR 0001: Phase 1 foundation decisions

Status: accepted for this foundation; security, platform and integration decisions remain subject to later validation.

## Context

Phase 0 separates RIT from Git, makes Git the source-history authority, and calls for a small Rust core with common interfaces, local-first operation and no feature placeholders. The user selected `https://github.com/RayAKaan/RIT` as the independent repository and confirmed `https://github.com/git/git.git` as reference.

## Decisions and alternatives

1. **Repository and workspace:** use the independent RIT repository and a two-crate Cargo workspace: `rit-core` and `rit-cli`. Alternative: one crate; rejected to enforce library/CLI dependency direction. Future crates appear with real functionality.
2. **Toolchain:** use stable 1.99.1 and edition 2024, pinned in `rust-toolchain.toml`, with 1.99.1 as the declared and tested minimum baseline. Older toolchains have not been tested; do not infer their support. Alternative: floating stable or older MSRV; rejected until compatibility is measured.
3. **Dependencies:** use clap for CLI, serde/toml for typed configuration, thiserror for source-preserving errors, tracing and tracing-subscriber for diagnostic logging, serde_json for JSON, tempfile in tests. Cargo.lock records resolution. No Tokio, UUID, database, Git or AI SDK. Typed domain identifiers are deferred until a Phase 2 entity needs one; Phase 0 specified opaque identifiers but did not approve a generation format, so inventing one now would be speculative.
4. **Config:** strict versioned TOML, defaults < optional discovered file < environment < CLI; fail on unknown fields and invalid lower layers. Explicit missing file is an error; never write the file. Alternative: silently ignoring unknown fields; rejected to avoid hiding mistakes.
5. **Errors:** typed core errors preserve sources; CLI maps them to documented exit categories, avoids printing raw parse sources, and directs human errors to stderr. No application-wide generic error framework is needed yet.
6. **Logging:** tracing subscriber initialized once in CLI, text or JSON on stderr; no global initialization in core. No distributed tracing or async runtime.
7. **Versioning:** `0.1.0-dev.1` is a pre-release workspace package version, surfaced via Cargo metadata. No invented commit metadata; no stability promise.
8. **Tests and CI:** Rust unit and real CLI subprocess tests, a locked CI matrix on Linux/macOS/Windows. Only Linux locally tested; workflows and platform support are not claimed green until observed.
9. **License:** no project license has been authorized. Packages are unpublished and `UNLICENSED`; owner must choose a license before distribution. Dependencies' licenses need review before release.

## Consequences and revisit conditions

Strict config fields require schema evolution. Pinned toolchain and locked dependencies support reproducibility but require deliberate updates. Future entities, services, stronger error categories and concurrency runtime should be introduced when needed. Review the MSRV, platform guarantees, dependency licenses and CI pins on upgrades. The Phase 0 publication and sandbox decisions remain provisional where noted there; this ADR does not resolve them.
