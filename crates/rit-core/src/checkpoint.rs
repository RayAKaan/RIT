//! Checkpoint metadata and non-destructive restore planning only.
use crate::{repository::RepositoryId, workspace::WorkspaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
/// Checkpoint identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointId(uuid::Uuid);
impl CheckpointId {
    /// Generate an identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::str::FromStr for CheckpointId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}
/// Commit-only reference. It does not capture staged, unstaged or untracked content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitCheckpoint {
    /// Stable ID derived from its Git ref.
    pub id: CheckpointId,
    /// Git reference under refs/rit/checkpoints.
    pub reference: String,
    /// Committed HEAD object captured.
    pub commit: crate::git_object::ObjectId,
    /// Explicit coverage limitation.
    pub capture_scope: String,
}
impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}
/// Named content reference; digest is metadata and must be verified by a future backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Git-style relative path; backend must validate against traversal and links.
    pub path: String,
    /// Content digest or object identifier, unverified by planner.
    pub content_id: String,
}
/// Checkpoint data does not itself imply a complete snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    /// ID.
    pub id: CheckpointId,
    /// Repository.
    pub repository: RepositoryId,
    /// Workspace.
    pub workspace: WorkspaceId,
    /// Base commit object ID.
    pub base_commit: String,
    /// Captured entries.
    pub entries: Vec<ManifestEntry>,
    /// Whether the provider claims complete coverage; still requires verification.
    pub claimed_complete: bool,
}
/// Preview only. No restore authorization or safety guarantee.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestorePreview {
    /// Exact manifest names requested.
    pub selected: Vec<String>,
    /// Names reported as newer/dirty by caller.
    pub conflicts: Vec<String>,
    /// Always false until restore safety is implemented.
    pub executable: bool,
}
impl Checkpoint {
    /// Produce a preview from caller-provided names; never opens or writes files.
    pub fn preview(
        &self,
        requested: &[String],
        dirty: &[String],
    ) -> Result<RestorePreview, &'static str> {
        let names: BTreeSet<_> = self.entries.iter().map(|e| e.path.as_str()).collect();
        if names.len() != self.entries.len() {
            return Err("duplicate manifest paths");
        }
        let selected: BTreeSet<_> = requested.iter().map(String::as_str).collect();
        if selected.iter().any(|p| !names.contains(p)) {
            return Err("requested path missing from manifest");
        }
        let dirty: BTreeSet<_> = dirty.iter().map(String::as_str).collect();
        Ok(RestorePreview {
            selected: selected.iter().map(|s| (*s).to_owned()).collect(),
            conflicts: selected
                .intersection(&dirty)
                .map(|s| (*s).to_owned())
                .collect(),
            executable: false,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_detects_conflict_and_never_enables_restore() {
        let c = Checkpoint {
            id: CheckpointId::new(),
            repository: RepositoryId::new(),
            workspace: WorkspaceId::new(),
            base_commit: "abc".into(),
            entries: vec![ManifestEntry {
                path: "a".into(),
                content_id: "hash".into(),
            }],
            claimed_complete: true,
        };
        let p = c.preview(&["a".into()], &["a".into()]).unwrap();
        assert_eq!(p.conflicts, vec!["a"]);
        assert!(!p.executable);
        assert!(c.preview(&["missing".into()], &[]).is_err());
    }
}
