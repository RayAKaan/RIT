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
/// Advisory candidate validation, never approval or permission to execute/publish.
#[derive(Serialize)]
pub struct CandidateValidation {
    /// Current candidate inventory and target movement.
    pub candidate: CandidateObservation,
    /// Bounded object-integrity check; may be unavailable.
    pub object_check: String,
    /// Partial static analysis; never proof of semantic safety.
    pub dependency_analysis: Option<rit_analysis::RepositoryGraph>,
    /// Failure explanation if partial analysis could not run.
    pub dependency_error: Option<String>,
    /// Repository builds/tests never executed.
    pub execution: &'static str,
    /// Publication remains unavailable.
    pub publication: &'static str,
}
/// Reconciled submission and related candidates; no approval claim.
#[derive(Serialize)]
pub struct SubmissionObservation {
    /// Persisted unauthenticated submission.
    pub submission: GitSubmission,
    /// Whether current source and target still match, or why they cannot be verified.
    pub source_target_check: String,
    /// Related candidate observations; may include interrupted work.
    pub candidates: Vec<CandidateObservation>,
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
    /// Cancel by expected-old-ref update; preserves all candidate/source data.
    pub fn cancel(&self, repo: &Path, id: SubmissionId) -> Result<GitSubmission, ApplicationError> {
        Ok(self.git.submission_cancel(repo, id)?)
    }
    /// Correlate existing submissions and candidates without modifying either.
    pub fn reconcile(&self, repo: &Path) -> Result<Vec<SubmissionObservation>, ApplicationError> {
        let submissions = self.git.submission_list(repo)?;
        let candidates = self.git.candidate_list(repo)?;
        Ok(submissions
            .into_iter()
            .map(|submission| {
                let check = if submission.lifecycle == "cancelled" {
                    "cancelled; source/target not checked".into()
                } else {
                    match self.git.submission_validate(repo, &submission) {
                        Ok(()) => "matches at observation time".into(),
                        Err(e) => format!("stale or unavailable: {e}"),
                    }
                };
                let related = candidates
                    .iter()
                    .filter(|c| c.candidate.submission == submission.id)
                    .cloned()
                    .collect();
                SubmissionObservation {
                    submission,
                    source_target_check: check,
                    candidates: related,
                }
            })
            .collect())
    }
    /// Re-run bounded read-only candidate checks without executing repository code.
    pub fn validate_candidate(
        &self,
        repo: &Path,
        id: rit_core::submission::CandidateId,
    ) -> Result<CandidateValidation, ApplicationError> {
        let candidate = self
            .git
            .candidate_list(repo)?
            .into_iter()
            .find(|x| x.candidate.id == id)
            .ok_or(rit_git::GitError::Parse("candidate not found"))?;
        if !matches!(candidate.observation.as_str(), "present" | "no_checkout") {
            return Err(rit_git::GitError::Unavailable(
                "candidate inventory or ref does not reconcile",
            )
            .into());
        }
        if candidate.target_moved {
            return Err(rit_git::GitError::Unavailable(
                "candidate target moved or cannot be verified; construct a fresh candidate",
            )
            .into());
        }
        self.git
            .candidate_verify_objects(repo, &candidate.candidate.commit)?;
        let object_check = "passed (Git fsck; auxiliary indexes excluded)".into();
        let graph = GitReadService::new(self.git.clone())
            .dependencies_revision(repo, candidate.candidate.commit.as_hex());
        let (dependency_analysis, dependency_error) = match graph {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Ok(CandidateValidation {
            candidate,
            object_check,
            dependency_analysis,
            dependency_error,
            execution: "NOT_RUN: no authorized execution boundary",
            publication: "NOT_AVAILABLE: no authenticated approval or protected publication",
        })
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
        self.git
            .candidate_verify_objects(repo, &candidate.candidate.commit)
            .map_err(
                |source| ApplicationError::CandidateCreatedValidationFailed {
                    id: candidate.candidate.id.to_string(),
                    source,
                },
            )?;
        let object_check = "passed (Git fsck; auxiliary indexes excluded)".into();
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
