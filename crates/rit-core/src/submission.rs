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
