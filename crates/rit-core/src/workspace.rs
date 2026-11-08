//! Workspace and isolation models; a Git worktree is not a security sandbox.
use crate::{repository::RepositoryId, security::PrincipalId};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
/// Opaque workspace identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(uuid::Uuid);
impl WorkspaceId {
    /// Generate an opaque identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for WorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for WorkspaceId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}
/// Workspace lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkspaceState {
    /// Planned, not created.
    Planned,
    /// Creation in progress.
    Creating,
    /// Available.
    Ready,
    /// Cleanup in progress.
    Removing,
    /// Removed.
    Removed,
    /// Operation failed; operator intervention may be required.
    Failed,
}
/// Isolation requested. Worktrees are coordination only.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IsolationProfile {
    /// Git worktree, not an isolation boundary.
    CooperativeWorktree,
    /// OS or container boundary, not yet validated.
    Hardened,
}
/// Resource ceilings requested by a caller; no enforcement is implied.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory in bytes.
    pub memory_bytes: Option<u64>,
    /// CPU time in seconds.
    pub cpu_seconds: Option<u64>,
    /// Wall clock limit in seconds.
    pub wall_seconds: Option<u64>,
}
/// Workspace metadata independent of its storage provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    /// Identifier.
    pub id: WorkspaceId,
    /// Related repository.
    pub repository: RepositoryId,
    /// Owning participant.
    pub owner: PrincipalId,
    /// Requested isolation.
    pub isolation: IsolationProfile,
    /// Lifecycle.
    pub state: WorkspaceState,
}
impl Workspace {
    /// Apply a safe lifecycle transition; this does not perform any filesystem operation.
    pub fn transition(&mut self, next: WorkspaceState) -> Result<(), &'static str> {
        use WorkspaceState::*;
        let allowed = matches!(
            (self.state, next),
            (Planned, Creating)
                | (Creating, Ready)
                | (Creating, Failed)
                | (Ready, Removing)
                | (Removing, Removed)
                | (Removing, Failed)
                | (Failed, Removing)
        );
        if !allowed {
            return Err("invalid workspace transition");
        }
        self.state = next;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_transitions() {
        let mut w = Workspace {
            id: WorkspaceId::new(),
            repository: RepositoryId::new(),
            owner: PrincipalId::new(),
            isolation: IsolationProfile::CooperativeWorktree,
            state: WorkspaceState::Planned,
        };
        assert!(w.transition(WorkspaceState::Ready).is_err());
        w.transition(WorkspaceState::Creating).unwrap();
        w.transition(WorkspaceState::Ready).unwrap();
        w.transition(WorkspaceState::Removing).unwrap();
        w.transition(WorkspaceState::Removed).unwrap();
        assert!(w.transition(WorkspaceState::Ready).is_err());
    }
}

/// Git-backed coordination metadata; never an authentication or authorization record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceManifest {
    /// Manifest schema.
    pub schema_version: u32,
    /// Opaque ID; also encoded in the branch and manifest ref names.
    pub id: WorkspaceId,
    /// Informational name, not a principal identity.
    pub name: String,
    /// Canonical requested worktree path.
    pub path: String,
    /// RIT-owned branch ref; ordinary `git push --all` may push it.
    pub branch: String,
    /// Base commit and object format.
    pub base: crate::git_object::ObjectId,
    /// Unix seconds at request time, not a trusted timestamp.
    pub created_at: u64,
    /// `provisioning`, `ready`, or `no_checkout`.
    pub state: String,
    /// Explicit materialization limitation, when any.
    pub limitation: Option<String>,
}
/// Reconciliation of untrusted manifest claims against live Git inventory.
#[derive(Clone, Debug, Serialize)]
pub struct ManagedWorkspaceObservation {
    /// Manifest content, not authenticated.
    pub manifest: WorkspaceManifest,
    /// Manifest object used for compare-and-swap.
    pub manifest_oid: crate::git_object::ObjectId,
    /// Actual Git inventory entry with matching branch, if any.
    pub worktree: Option<crate::git_object::WorktreeRecord>,
    /// Reconciliation classification.
    pub observation: String,
    /// Live Git status if safely available.
    pub status: Option<crate::repository::Status>,
}
/// Bounded workspace reconciliation result.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceReconciliation {
    /// Valid manifest observations.
    pub workspaces: Vec<ManagedWorkspaceObservation>,
    /// RIT branches with no matching manifest; never deleted automatically.
    pub orphan_branches: Vec<String>,
    /// RIT branches checked out without a valid manifest.
    pub unclaimed_worktrees: Vec<String>,
}
