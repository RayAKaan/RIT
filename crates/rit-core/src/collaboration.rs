//! Task and review metadata, without a durable collaboration service.
use crate::{
    repository::RepositoryId, security::PrincipalId, submission::SubmissionId,
    workspace::WorkspaceId,
};
use serde::{Deserialize, Serialize};
/// Task identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(uuid::Uuid);
impl TaskId {
    /// Generate an opaque identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}
/// Task lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TaskState {
    /// Not started.
    Open,
    /// Work underway.
    Active,
    /// Proposed completion.
    InReview,
    /// Work finished.
    Completed,
    /// Work cancelled.
    Cancelled,
}
/// Participant association, not an authorization grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant {
    /// Principal ID.
    pub principal: PrincipalId,
    /// Descriptive role label, not policy.
    pub role: String,
}
/// Task metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    /// ID.
    pub id: TaskId,
    /// Repository.
    pub repository: RepositoryId,
    /// Workspaces.
    pub workspaces: Vec<WorkspaceId>,
    /// Participants.
    pub participants: Vec<Participant>,
    /// Lifecycle.
    pub state: TaskState,
}
impl Task {
    /// Validate a pure lifecycle transition; this is not an authorized operation.
    pub fn transition(&mut self, next: TaskState) -> Result<(), &'static str> {
        use TaskState::*;
        if !matches!(
            (self.state, next),
            (Open, Active)
                | (Open, Cancelled)
                | (Active, InReview)
                | (Active, Cancelled)
                | (InReview, Active)
                | (InReview, Completed)
                | (InReview, Cancelled)
        ) {
            return Err("invalid task transition");
        }
        self.state = next;
        Ok(())
    }
}
/// Review record. It is not an authenticated or durable approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    /// Related submission.
    pub submission: SubmissionId,
    /// Reviewer identity is unverified metadata.
    pub reviewer: PrincipalId,
    /// Informational decision only.
    pub decision: ReviewDecision,
}
/// Review recommendation. A real approval must be authenticated and bound to candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    /// Informational approval.
    Approve,
    /// Changes requested.
    RequestChanges,
    /// Informational comment.
    Comment,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tasks_do_not_skip_review() {
        let mut t = Task {
            id: TaskId::new(),
            repository: RepositoryId::new(),
            workspaces: vec![],
            participants: vec![],
            state: TaskState::Open,
        };
        assert!(t.transition(TaskState::Completed).is_err());
        t.transition(TaskState::Active).unwrap();
        t.transition(TaskState::InReview).unwrap();
        t.transition(TaskState::Completed).unwrap();
        assert!(t.transition(TaskState::Active).is_err());
    }
}
