//! RIT's reusable configuration and version foundation.
//!
//! This crate does not perform Git, database, network, or global logging operations.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
/// Repository identity, inspection, and status domain types.
pub mod repository;
pub mod version;
