//! Typed, read-only Git inspection adapter.
#![forbid(unsafe_code)]
mod process;
mod status;
pub use process::{Git, GitError};
