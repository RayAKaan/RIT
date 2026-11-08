//! Submission metadata and advisory integration checks; never updates refs.
use crate::{repository::RepositoryId, security::PrincipalId, workspace::WorkspaceId};
use serde::{Deserialize, Serialize};
/// Durable submission identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubmissionId(uuid::Uuid);
impl SubmissionId {
    /// Generate an opaque ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for SubmissionId {
    fn default() -> Self {
        Self::new()
    }
}
/// Integration state. An actual transition must be coordinated by a durable service.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SubmissionState {
    /// In preparation.
    Draft,
    /// Awaiting integration.
    Queued,
    /// Building candidate.
    Preparing,
    /// Checks running.
    Validating,
    /// Awaiting authorized approval.
    AwaitingApproval,
    /// Completed by a verified provider.
    Published,
    /// Needs attention.
    Failed,
}
/// Submission input. Git remains authoritative for object/ref contents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submission {
    /// ID.
    pub id: SubmissionId,
    /// Repository.
    pub repository: RepositoryId,
    /// Source workspace.
    pub workspace: WorkspaceId,
    /// Submitter identity (not self-authenticating).
    pub author: PrincipalId,
    /// Source commit.
    pub source_commit: String,
    /// Base commit.
    pub base_commit: String,
    /// Expected target ref value for compare-and-swap.
    pub expected_target: String,
    /// Candidate commit, if built.
    pub candidate: Option<String>,
    /// State.
    pub state: SubmissionState,
}
/// An advisory check, never permission to publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationPreview {
    /// Whether observed target matches caller's expectation.
    pub target_matches: bool,
    /// Whether a candidate was supplied.
    pub candidate_present: bool,
    /// Publication is not implemented.
    pub executable: bool,
}
impl Submission {
    /// Compare supplied inputs without touching Git or storage.
    pub fn preview(&self, observed_target: &str) -> IntegrationPreview {
        IntegrationPreview {
            target_matches: self.expected_target == observed_target,
            candidate_present: self.candidate.is_some(),
            executable: false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_target_does_not_enable_publish() {
        let s = Submission {
            id: SubmissionId::new(),
            repository: RepositoryId::new(),
            workspace: WorkspaceId::new(),
            author: PrincipalId::new(),
            source_commit: "s".into(),
            base_commit: "b".into(),
            expected_target: "t".into(),
            candidate: Some("c".into()),
            state: SubmissionState::Queued,
        };
        assert!(!s.preview("other").target_matches);
        assert!(!s.preview("t").executable);
    }
}
impl std::fmt::Display for SubmissionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::str::FromStr for SubmissionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}
/// Candidate coordination ID; not an authenticated approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(uuid::Uuid);
impl CandidateId {
    /// Generate a random identity.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for CandidateId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for CandidateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::str::FromStr for CandidateId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}
/// Unauthenticated Git-ref-backed submission, not an approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSubmission {
    /// Manifest schema.
    pub schema_version: u32,
    /// Submission identity.
    pub id: SubmissionId,
    /// Source workspace coordination ID.
    pub workspace: WorkspaceId,
    /// Captured RIT branch.
    pub source_branch: String,
    /// Captured source commit.
    pub source_commit: crate::git_object::ObjectId,
    /// Workspace creation base.
    pub base_commit: crate::git_object::ObjectId,
    /// Full target branch ref.
    pub target_ref: String,
    /// Target commit observed at submission.
    pub target_commit: crate::git_object::ObjectId,
    /// Changed paths relative to workspace base; no rename inference.
    pub changed_files: Vec<String>,
    /// Explicitly submitted commit only; ignored content and future edits are not included.
    pub scope: String,
}
/// Git-ref-backed candidate metadata, not a protected approval or publication record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitCandidate {
    /// Schema version.
    pub schema_version: u32,
    /// Candidate identity.
    pub id: CandidateId,
    /// Associated unauthenticated submission.
    pub submission: SubmissionId,
    /// Commit containing the merged tree, target first and source second parent.
    pub commit: crate::git_object::ObjectId,
    /// Source commit.
    pub source_commit: crate::git_object::ObjectId,
    /// Target ref at candidate creation.
    pub target_ref: String,
    /// Target commit expected at submission.
    pub target_commit: crate::git_object::ObjectId,
    /// Detached candidate worktree path.
    pub path: String,
    /// `provisioning`, `ready` or `no_checkout`.
    pub state: String,
    /// Materialization limitation if any.
    pub limitation: Option<String>,
}
/// Candidate manifest reconciled against Git, with no authorization inference.
#[derive(Clone, Debug, Serialize)]
pub struct CandidateObservation {
    /// Manifest record.
    pub candidate: GitCandidate,
    /// Current worktree if present and matching.
    pub worktree: Option<crate::git_object::WorktreeRecord>,
    /// Present, no_checkout, moved, missing, interrupted or mismatch.
    pub observation: String,
    /// Whether the target ref no longer points at the captured target commit.
    pub target_moved: bool,
}
