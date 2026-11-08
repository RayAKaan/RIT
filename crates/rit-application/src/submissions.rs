//! Unauthenticated Git-backed submissions and non-publishing candidate construction.
use crate::{
    ApplicationError, GitReadService,
    conflicts::{ConflictReport, ConflictService},
};
use rit_core::{
    submission::{CandidateObservation, GitSubmission, SubmissionId},
    workspace::WorkspaceId,
};
use rit_git::Git;
use serde::Serialize;
use std::path::Path;

/// Submission receipt plus ephemeral cross-phase evidence.
#[derive(Serialize)]
pub struct SubmissionReceipt {
    /// Durable Git-ref-backed submission record.
    pub submission: GitSubmission,
    /// Partial dependency graph for the captured source commit, if available.
    pub dependency_analysis: Option<Box<rit_analysis::RepositoryGraph>>,
    /// Diagnostics if analysis could not be completed.
    pub dependency_error: Option<String>,
    /// IDs of available content checkpoints for this workspace, if listing succeeded.
    pub related_checkpoints: Option<Vec<String>>,
    /// Advisory evidence is ephemeral and is not authenticated or an approval.
    pub limitations: Vec<&'static str>,
}
/// Candidate outcome with explicit non-execution and incomplete analysis.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CandidateResult {
    /// Real Git merge conflicts; no candidate worktree was created.
    Conflicted {
        /// Ephemeral evidence.
        conflict_report: ConflictReport,
    },
    /// A detached candidate was created without publishing the target.
    Created {
        /// Reconciled candidate.
        candidate: Box<CandidateObservation>,
        /// Pre-creation temporary-object merge evidence.
        conflict_report: ConflictReport,
        /// Partial static dependency graph of candidate commit, if available.
        dependency_analysis: Option<Box<rit_analysis::RepositoryGraph>>,
        /// Analysis failure rather than a fabricated pass.
        dependency_error: Option<String>,
        /// Limited Git fsck result.
        object_check: String,
        /// Tests/builds never executed.
        execution: &'static str,
        /// This candidate is neither authorized nor published.
        publication: &'static str,
    },
}
/// Service backing CLI and potential scoped adapters.
pub struct SubmissionService {
    git: Git,
}
impl SubmissionService {
    /// Construct a service.
    pub fn new(git: Git) -> Self {
        Self { git }
    }
    /// Capture a committed source and target; analysis is advisory and ephemeral.
    pub fn create(
        &self,
        repo: &Path,
        workspace: WorkspaceId,
        target: &str,
    ) -> Result<SubmissionReceipt, ApplicationError> {
        let submission = self.git.submission_create(repo, workspace, target)?;
        let analysis = GitReadService::new(self.git.clone())
            .dependencies_revision(repo, submission.source_commit.as_hex());
        let (dependency_analysis, dependency_error) = match analysis {
            Ok(v) => (Some(Box::new(v)), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let related_checkpoints = self.git.content_checkpoint_list(repo).ok().map(|list| {
            list.into_iter()
                .filter(|c| c.manifest.workspace == workspace)
                .map(|c| c.manifest.id.to_string())
                .collect()
        });
        Ok(SubmissionReceipt {
            submission,
            dependency_analysis,
            dependency_error,
            related_checkpoints,
            limitations: vec![
                "Git refs and manifests are unauthenticated",
                "dependency analysis is partial and ephemeral",
                "publication and approval are unavailable",
            ],
        })
    }
    /// List persisted unauthenticated submissions.
    pub fn list(&self, repo: &Path) -> Result<Vec<GitSubmission>, ApplicationError> {
        Ok(self.git.submission_list(repo)?)
    }
    /// List and reconcile candidate metadata with Git worktree inventory.
    pub fn candidates(&self, repo: &Path) -> Result<Vec<CandidateObservation>, ApplicationError> {
        Ok(self.git.candidate_list(repo)?)
    }
    /// Preview conflicts, construct if clean, and perform bounded static validation.
    pub fn candidate(
        &self,
        repo: &Path,
        id: SubmissionId,
        path: &Path,
    ) -> Result<CandidateResult, ApplicationError> {
        let item = self
            .git
            .submission_list(repo)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or(rit_git::GitError::Parse("submission not found"))?;
        self.git.submission_validate(repo, &item)?;
        let report = ConflictService::new(self.git.clone()).simulate(
            repo,
            item.target_commit.as_hex(),
            item.source_commit.as_hex(),
        )?;
        if report.simulation.has_conflicts {
            return Ok(CandidateResult::Conflicted {
                conflict_report: report,
            });
        }
        let candidate = self.git.candidate_create(repo, id, path)?;
        let analysis = GitReadService::new(self.git.clone())
            .dependencies_revision(repo, candidate.candidate.commit.as_hex());
        let (dependency_analysis, dependency_error) = match analysis {
            Ok(v) => (Some(Box::new(v)), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let object_check = match self
            .git
            .candidate_verify_objects(repo, &candidate.candidate.commit)
        {
            Ok(()) => "passed (Git fsck; auxiliary indexes excluded)".into(),
            Err(e) => format!("unavailable or failed: {e}"),
        };
        Ok(CandidateResult::Created {
            candidate: Box::new(candidate),
            conflict_report: report,
            dependency_analysis,
            dependency_error,
            object_check,
            execution: "not run: no authorized execution boundary",
            publication: "not published; approval and publication are unavailable",
        })
    }
}
