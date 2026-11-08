//! Typed repository observations and local registry entities.
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Opaque, random repository identity, independent of filesystem path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(uuid::Uuid);
impl RepositoryId {
    /// Generate a random version-4 identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for RepositoryId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for RepositoryId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}

/// Git HEAD observation; an unborn branch has no commit yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Head {
    /// A symbolic branch with an existing commit.
    Branch {
        /// Branch name.
        name: String,
        /// Full object ID.
        commit: String,
    },
    /// A detached commit.
    Detached {
        /// Full object ID.
        commit: String,
    },
    /// A symbolic branch that has not received its first commit.
    Unborn {
        /// Branch name.
        name: String,
    },
}
/// One changed path in porcelain-v2 status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Record kind: ordinary, renamed, unmerged, or untracked.
    pub kind: String,
    /// UTF-8 path relative to the working tree root.
    pub path: String,
    /// Original path for renames/copies.
    pub original_path: Option<String>,
    /// Git XY index/worktree status.
    pub xy: Option<String>,
    /// Git submodule status field, if tracked.
    pub submodule: Option<String>,
}
/// Live Git working-tree status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Branch HEAD name or `(detached)`.
    pub branch: Option<String>,
    /// Full HEAD OID, or `(initial)` for an unborn branch.
    pub oid: Option<String>,
    /// Configured upstream branch, if any.
    pub upstream: Option<String>,
    /// Commits ahead of upstream when reported.
    pub ahead: Option<u64>,
    /// Commits behind upstream when reported.
    pub behind: Option<u64>,
    /// Changed paths.
    pub changes: Vec<Change>,
}
/// Live repository observation. Paths are validated UTF-8, never lossy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inspection {
    /// Input location, canonicalized.
    pub location: String,
    /// Working-tree root, absent for bare repositories.
    pub worktree: Option<String>,
    /// Per-worktree Git directory.
    pub git_dir: String,
    /// Shared Git directory, used for local deduplication.
    pub common_git_dir: String,
    /// Whether the observed location is bare.
    pub bare: bool,
    /// Git-reported object format.
    pub object_format: String,
    /// Git shallow-repository flag.
    pub shallow: bool,
    /// Live HEAD state.
    pub head: Head,
    /// Remote names, never URLs.
    pub remotes: Vec<String>,
    /// Live working-tree status; absent for bare repositories.
    pub status: Option<Status>,
}
/// Persistent RIT registry record, not an authority on live Git content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repository {
    /// Stable local identity.
    pub id: RepositoryId,
    /// User-facing name.
    pub display_name: String,
    /// Canonical common Git directory at registration time.
    pub common_git_dir: String,
    /// Object format at registration time.
    pub object_format: String,
    /// Whether the first observed location was bare.
    pub is_bare: bool,
    /// Unix time when created.
    pub created_at: i64,
    /// Unix time when last registered.
    pub updated_at: i64,
    /// All known observed locations.
    pub locations: Vec<Location>,
}
/// One observed checkout or bare location of a registered repository.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    /// Canonical location path.
    pub path: String,
    /// Per-location Git directory.
    pub git_dir: String,
    /// `worktree` or `bare`.
    pub kind: String,
    /// Unix time when first observed.
    pub first_seen_at: i64,
    /// Unix time when last observed.
    pub last_seen_at: i64,
}
