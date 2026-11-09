//! Typed Git inspection adapter with explicitly scoped commit-only checkpoint refs.
#![forbid(unsafe_code)]
mod process;
mod status;
pub use process::{Git, GitError};
