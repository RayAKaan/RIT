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

/// One staged index entry captured by a content checkpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedIndexEntry {
    /// Repository-relative path.
    pub path: String,
    /// Git index mode.
    pub mode: String,
    /// Index conflict stage (zero for normal entries).
    pub stage: u8,
    /// Existing index object ID; gitlinks may refer to a different object database.
    pub object: crate::git_object::ObjectId,
}
/// Raw file or symlink content captured from a worktree.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedWorktreeEntry {
    /// Repository-relative path.
    pub path: String,
    /// `tracked`, `untracked`, or `selected_ignored`.
    pub category: String,
    /// `regular`, `symlink`, or `deleted`.
    pub kind: String,
    /// POSIX executable bit for regular files.
    pub executable: bool,
    /// Git blob holding exact raw file bytes or link target; absent for deletion.
    pub blob: Option<crate::git_object::ObjectId>,
}
fn unknown_consistency() -> String {
    "not_recorded".into()
}
/// Explicit capture budgets. Hard provider ceilings remain in force.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureLimits {
    /// Maximum selected worktree paths, up to 2,000.
    pub max_paths: usize,
    /// Maximum raw bytes in one file, up to 4 MiB.
    pub max_file_bytes: usize,
    /// Maximum total raw worktree bytes, up to 32 MiB.
    pub max_total_bytes: usize,
}
impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_paths: 500,
            max_file_bytes: 1_048_576,
            max_total_bytes: 8_388_608,
        }
    }
}
impl CaptureLimits {
    /// Validate against bounded provider ceilings.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=2000).contains(&self.max_paths)
            || !(1..=4_194_304).contains(&self.max_file_bytes)
            || !(1..=33_554_432).contains(&self.max_total_bytes)
            || self.max_file_bytes > self.max_total_bytes
        {
            return Err(
                "checkpoint budgets must be positive, within provider ceilings, and file budget must not exceed total budget",
            );
        }
        Ok(())
    }
}
/// Unauthenticated Git-object coordination data, not a restore authorization.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCheckpoint {
    /// Format version.
    pub schema_version: u32,
    /// Checkpoint identity.
    pub id: CheckpointId,
    /// Workspace identity, coordination only.
    pub workspace: WorkspaceId,
    /// Base commit captured before and checked again after capture.
    pub base: crate::git_object::ObjectId,
    /// Git blob of the original index bytes.
    pub index_blob: crate::git_object::ObjectId,
    /// Original index entries and stages.
    pub staged: Vec<CapturedIndexEntry>,
    /// Exact raw worktree content for observed paths.
    pub worktree: Vec<CapturedWorktreeEntry>,
    /// Optional capture budgets, present on schema 3 records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_limits: Option<CaptureLimits>,
    /// Untrusted Unix creation time; absent from earlier schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    /// Whether nonignored untracked files were selected; absent from earlier schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_untracked: Option<bool>,
    /// Observation consistency, not an atomic snapshot or authenticated claim.
    #[serde(default = "unknown_consistency")]
    pub consistency: String,
    /// Explicit categories and state that cannot be guaranteed complete.
    pub limitations: Vec<String>,
    /// Always false: capture spans time and cannot guarantee complete atomic coverage.
    pub complete: bool,
    /// Explicit source and security limitations.
    pub scope: String,
}
/// Content checkpoint ref and its validated manifest, not authenticated provenance.
#[derive(Clone, Debug, Serialize)]
pub struct ContentCheckpointRecord {
    /// RIT-owned ref, with expected-zero creation.
    pub reference: String,
    /// Git commit retaining snapshot objects and base history.
    pub object: crate::git_object::ObjectId,
    /// Content manifest.
    pub manifest: ContentCheckpoint,
}

/// Opaque restore-operation ID; not an authenticated identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RestoreId(uuid::Uuid);
impl RestoreId {
    /// Generate an operation ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for RestoreId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for RestoreId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::str::FromStr for RestoreId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}
/// A read-only plan for restoring only captured data into a new worktree.
#[derive(Clone, Debug, Serialize)]
pub struct RestorePlan {
    /// Unauthenticated source checkpoint and its declared limitations.
    pub checkpoint: ContentCheckpointRecord,
    /// New, currently absent worktree path.
    pub destination: String,
    /// Paths for which a regular file or symlink will be created.
    pub create_paths: Vec<String>,
    /// Recorded missing tracked paths, intentionally left absent.
    pub deleted_paths: Vec<String>,
    /// Explicit limitations; the checkpoint is non-atomic and may omit user content.
    pub limitations: Vec<String>,
}
/// Durable cooperative recovery intent; not authorization or a trusted audit record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreOperation {
    /// Schema.
    pub schema_version: u32,
    /// Restore operation identity.
    pub id: RestoreId,
    /// Checkpoint identity.
    pub checkpoint: CheckpointId,
    /// Exact checkpoint commit used; the retained ref pins it.
    pub checkpoint_object: crate::git_object::ObjectId,
    /// Base HEAD for the new detached worktree.
    pub base: crate::git_object::ObjectId,
    /// Expected new worktree path.
    pub destination: String,
    /// Last durably recorded step; actual filesystem can be ahead.
    pub state: String,
    /// Successfully written paths at the time of the last CAS update.
    pub completed_paths: Vec<String>,
    /// Path currently being attempted, if a crash occurred before it was recorded complete.
    pub current_path: Option<String>,
    /// Failure explanation, when record update succeeded.
    pub failure: Option<String>,
    /// No automatic overwrite or cleanup is allowed.
    pub action: String,
}
/// Live inventory observation of a restore operation.
#[derive(Clone, Debug, Serialize)]
pub struct RestoreObservation {
    /// Unauthenticated recovery intent.
    pub operation: RestoreOperation,
    /// Manifest object identity.
    pub object: crate::git_object::ObjectId,
    /// Current Git inventory evidence.
    pub worktree: Option<crate::git_object::WorktreeRecord>,
    /// Completed inventory is not a guarantee that files have not subsequently changed.
    pub observation: String,
}

/// LFS pointer found in captured staged/worktree bytes, not the payload itself.
#[derive(Clone, Debug, Serialize)]
pub struct LfsPointerEvidence {
    /// Captured workspace-relative path.
    pub path: String,
    /// Staged index stage or worktree observation.
    pub source: String,
    /// SHA-256 pointer OID when valid.
    pub sha256: Option<String>,
    /// Declared payload size when valid.
    pub size: Option<u64>,
    /// Local payload observation; the payload is not retained by the checkpoint.
    pub local_status: String,
}
/// Gitlink object ID is not a nested checkpoint.
#[derive(Clone, Debug, Serialize)]
pub struct GitlinkEvidence {
    /// Workspace-relative submodule path.
    pub path: String,
    /// Index stage.
    pub stage: u8,
    /// Captured gitlink ID.
    pub commit: crate::git_object::ObjectId,
    /// Nested Git repository and content availability are not checked.
    pub availability: &'static str,
}
/// Supplemental current availability evidence, not capture completeness or provenance.
#[derive(Clone, Debug, Serialize)]
pub struct CheckpointSupplementaryEvidence {
    /// Recognized pointers among captured blob candidates up to 2048 bytes.
    pub lfs_pointers: Vec<LfsPointerEvidence>,
    /// Gitlinks without nested-content guarantees.
    pub gitlinks: Vec<GitlinkEvidence>,
    /// Limits and absence of nested-content preservation.
    pub limitations: Vec<&'static str>,
}

/// Preview of selected captured worktree content exported to an absent directory.
#[derive(Clone, Debug, Serialize)]
pub struct ExtractionPlan {
    /// Checkpoint and explicitly incomplete coverage.
    pub checkpoint: ContentCheckpointRecord,
    /// New destination directory, outside all known worktrees.
    pub destination: String,
    /// Captured file or symlink paths selected by the user.
    pub selected_paths: Vec<String>,
    /// Captured deletions under the selection; no path is deleted.
    pub recorded_deletions: Vec<String>,
    /// This exports raw worktree content, not index or Git operation state.
    pub limitations: Vec<String>,
}
/// Cooperative extraction intent; not authenticated and not a filesystem transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionOperation {
    /// Schema version.
    pub schema_version: u32,
    /// Opaque operation identifier.
    pub id: RestoreId,
    /// Referenced checkpoint ID.
    pub checkpoint: CheckpointId,
    /// Exact pinned checkpoint commit.
    pub checkpoint_object: crate::git_object::ObjectId,
    /// New destination directory.
    pub destination: String,
    /// Selected captured paths.
    pub selected_paths: Vec<String>,
    /// Paths durably recorded as written; actual filesystem may be ahead.
    pub completed_paths: Vec<String>,
    /// Path currently being written, if any.
    pub current_path: Option<String>,
    /// Last persisted state.
    pub state: String,
    /// Failure if its metadata update succeeded.
    pub failure: Option<String>,
    /// Safe operator action.
    pub action: String,
}
/// Extraction state reconciled against current directory presence only.
#[derive(Clone, Debug, Serialize)]
pub struct ExtractionObservation {
    /// Unauthenticated operation record.
    pub operation: ExtractionOperation,
    /// CAS object identity.
    pub object: crate::git_object::ObjectId,
    /// Inventory-only classification; no current content verification is implied.
    pub observation: String,
}
