//! RIT's reusable configuration and version foundation.
//!
//! This crate does not perform Git, database, network, or global logging operations.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Snapshot activity reconciliation.
pub mod activity;
/// Checkpoint metadata and preview.
pub mod checkpoint;
/// Task and review domain types.
pub mod collaboration;
pub mod config;
/// Read-only conflict evidence.
pub mod conflict;
pub mod error;
/// Algorithm-aware Git object identifiers and observations.
pub mod git_object;
/// Explicitly partial dependency graph and impact traversal.
pub mod impact;
/// Repository identity, inspection, and status domain types.
pub mod repository;
/// Pure identity, policy and audit models; not an authentication implementation.
pub mod security;
/// Submission and integration preview types.
pub mod submission;
pub mod version;
/// Workspace lifecycle and isolation types.
pub mod workspace;
