//! Build version sourced from the Cargo package metadata.

/// The RIT core package version; workspace package versions are kept in sync.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
