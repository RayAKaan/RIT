//! Application services coordinating independent Git and registry adapters.
#![forbid(unsafe_code)]
/// Security and later-phase provider contracts.
pub mod contracts;
use rit_core::repository::{Inspection, Repository, RepositoryId};
use rit_git::{Git, GitError};
use rit_storage::{Registry, StorageError};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Repository workflow failures.
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// Git inspection failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// Registry operation failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Repository source analysis failed.
    #[error(transparent)]
    Analysis(#[from] rit_analysis::AnalysisError),
    /// Repository was not registered.
    #[error("repository is not registered; use rit repository register")]
    NotRegistered,
    /// Registry location no longer exists.
    #[error(
        "registered repository location is unavailable; relocation management is not yet supported"
    )]
    StaleLocation,
    /// Storage must not be written inside an observed user repository.
    #[error("data directory must be outside the Git repository and its Git directory")]
    DataInsideRepository,
    /// Resolving the data directory failed.
    #[error("cannot resolve data directory: {0}")]
    DataPath(#[source] std::io::Error),
}
/// A repository workflow scoped to a Git executable and one local registry location.
pub struct Repositories {
    git: Git,
    data_dir: PathBuf,
}
impl Repositories {
    /// Compose the service without touching disk.
    pub fn new(git: Git, data_dir: PathBuf) -> Self {
        Self { git, data_dir }
    }
    /// Inspect Git without opening the registry.
    pub fn inspect(&self, path: &Path) -> Result<Inspection, ApplicationError> {
        Ok(self.git.inspect(path)?)
    }
    /// Register an existing repository, never creating a Git repository.
    pub fn register(
        &self,
        path: &Path,
        name: Option<&str>,
    ) -> Result<(Repository, bool), ApplicationError> {
        let ins = self.inspect(path)?;
        let data = resolve_future_path(&self.data_dir)?;
        if [&ins.location, &ins.common_git_dir, &ins.git_dir]
            .iter()
            .any(|p| data.starts_with(p) || Path::new(p).starts_with(&data))
        {
            return Err(ApplicationError::DataInsideRepository);
        }
        Ok(Registry::open(&self.data_dir)?.register(&ins, name)?)
    }
    /// List records and their current filesystem-only path status without invoking Git.
    pub fn list(&self) -> Result<Vec<RepositoryListing>, ApplicationError> {
        match Registry::open_readonly(&self.data_dir)? {
            Some(registry) => Ok(annotate_locations(registry.list()?)),
            None => Ok(Vec::new()),
        }
    }
    /// Find a registered repository by full ID or by an observed path.
    pub fn find(&self, selector: &str) -> Result<Repository, ApplicationError> {
        let registry =
            Registry::open_readonly(&self.data_dir)?.ok_or(ApplicationError::NotRegistered)?;
        if let Ok(id) = selector.parse::<RepositoryId>() {
            return registry.get(id)?.ok_or(ApplicationError::NotRegistered);
        }
        let ins = self.git.inspect(Path::new(selector))?;
        registry
            .list()?
            .into_iter()
            .find(|r| r.common_git_dir == ins.common_git_dir)
            .ok_or(ApplicationError::NotRegistered)
    }
    /// Inspect a registered repository at the selected or a still-existing location.
    pub fn live(&self, selector: &str) -> Result<(Repository, Inspection), ApplicationError> {
        let r = self.find(selector)?;
        if selector.parse::<RepositoryId>().is_ok() {
            // Stored order is stable. A missing, inaccessible, repurposed or otherwise
            // invalid location cannot mask a later location that still matches.
            for location in &r.locations {
                if let Ok(ins) = self.git.inspect(Path::new(&location.path))
                    && ins.common_git_dir == r.common_git_dir
                {
                    return Ok((r, ins));
                }
            }
            Err(ApplicationError::StaleLocation)
        } else {
            let ins = self.git.inspect(Path::new(selector))?;
            if ins.common_git_dir != r.common_git_dir {
                return Err(ApplicationError::StaleLocation);
            }
            Ok((r, ins))
        }
    }
}
// Resolve the nearest existing ancestor to catch symlinks, even before a new directory exists.
fn resolve_future_path(path: &Path) -> Result<PathBuf, ApplicationError> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ApplicationError::DataPath(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "data directory must not contain parent traversal",
        )));
    }
    let p = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(ApplicationError::DataPath)?
            .join(path)
    };
    let mut ancestor = p.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            ApplicationError::DataPath(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid directory",
            ))
        })?;
        suffix.push(name.to_owned());
        ancestor = ancestor.parent().ok_or_else(|| {
            ApplicationError::DataPath(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid directory",
            ))
        })?;
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(ApplicationError::DataPath)?;
    for part in suffix.iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

/// A persisted repository accompanied by non-authoritative current path observations.
#[derive(serde::Serialize)]
pub struct RepositoryListing {
    /// Persisted record; it is not live Git truth.
    #[serde(flatten)]
    pub repository: Repository,
    /// Current filesystem-only state of each recorded location, in matching order.
    pub location_availability: Vec<LocationAvailability>,
}
/// Coarse path status; present paths have not been verified as the same repository.
#[derive(serde::Serialize)]
pub struct LocationAvailability {
    /// Recorded location path.
    pub path: String,
    /// `present_unverified`, `missing`, `not_directory`, or `inaccessible`.
    pub state: &'static str,
}
/// Attach current filesystem-only status without invoking Git or modifying the registry.
pub fn annotate_locations(repositories: Vec<Repository>) -> Vec<RepositoryListing> {
    repositories
        .into_iter()
        .map(|repository| {
            let location_availability = repository
                .locations
                .iter()
                .map(|location| {
                    let state = match std::fs::metadata(&location.path) {
                        Ok(m) if m.is_dir() => "present_unverified",
                        Ok(_) => "not_directory",
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
                        Err(_) => "inaccessible",
                    };
                    LocationAvailability {
                        path: location.path.clone(),
                        state,
                    }
                })
                .collect();
            RepositoryListing {
                repository,
                location_availability,
            }
        })
        .collect()
}

/// Direct read-only Git application service; does not access a RIT registry or authenticate actors.
pub struct GitReadService {
    git: Git,
}
impl GitReadService {
    /// Construct a service from the configured Git adapter.
    pub fn new(git: Git) -> Self {
        Self { git }
    }
    /// Inspect an explicit Git repository path.
    pub fn inspect(&self, path: &Path) -> Result<Inspection, ApplicationError> {
        Ok(self.git.inspect(path)?)
    }
    /// Analyze supported committed source blobs; no working-tree or registry access.
    pub fn dependencies(
        &self,
        path: &Path,
    ) -> Result<rit_analysis::RepositoryGraph, ApplicationError> {
        Ok(rit_analysis::repository_graph(
            self.git.source_snapshot(path)?,
        )?)
    }
    /// Analyze a selected immutable Git commit or branch, without worktree overlay.
    pub fn dependencies_revision(
        &self,
        path: &Path,
        revision: &str,
    ) -> Result<rit_analysis::RepositoryGraph, ApplicationError> {
        Ok(rit_analysis::repository_graph(
            self.git.source_snapshot_revision(path, Some(revision))?,
        )?)
    }
    /// Analyze current worktree content; this is a non-atomic, incomplete observation.
    pub fn dependencies_worktree(
        &self,
        path: &Path,
    ) -> Result<rit_analysis::RepositoryGraph, ApplicationError> {
        Ok(rit_analysis::repository_graph(
            self.git.source_snapshot_worktree(path)?,
        )?)
    }
    /// Analyze an identified managed worktree after reconciling it against Git inventory.
    pub fn dependencies_workspace(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
    ) -> Result<rit_analysis::RepositoryGraph, ApplicationError> {
        let state = self.git.workspace_reconcile(repo)?;
        let w = state
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == id)
            .ok_or(ApplicationError::Git(GitError::Parse(
                "workspace not found",
            )))?;
        if w.observation != "present" {
            return Err(ApplicationError::Git(GitError::Unavailable(
                "workspace is not present and ready",
            )));
        }
        self.dependencies_worktree(Path::new(&w.manifest.path))
    }
    /// Obtain the tracked and untracked working-tree status.
    pub fn status(&self, path: &Path) -> Result<rit_core::repository::Status, ApplicationError> {
        self.inspect(path)?
            .status
            .ok_or(ApplicationError::Git(GitError::Parse(
                "bare repository has no working tree",
            )))
    }
    /// Obtain refs.
    pub fn references(
        &self,
        path: &Path,
    ) -> Result<Vec<rit_core::git_object::GitReference>, ApplicationError> {
        Ok(self.git.references(path)?)
    }
    /// Obtain bounded HEAD history.
    pub fn history(
        &self,
        path: &Path,
        limit: u32,
    ) -> Result<Vec<rit_core::git_object::GitCommit>, ApplicationError> {
        Ok(self.git.history(path, limit)?)
    }
    /// Obtain Git worktree inventory.
    pub fn worktrees(
        &self,
        path: &Path,
    ) -> Result<Vec<rit_core::git_object::WorktreeRecord>, ApplicationError> {
        Ok(self.git.worktrees(path)?)
    }
    /// Obtain staged and unstaged tracked file differences.
    pub fn diff(
        &self,
        path: &Path,
    ) -> Result<rit_core::git_object::DiffObservation, ApplicationError> {
        Ok(self.git.diff(path)?)
    }
}

/// Explicit commit-only checkpoint service; it does not capture uncommitted work or use the registry.
pub struct GitCheckpointService {
    git: Git,
}
impl GitCheckpointService {
    /// Construct a service with the configured Git adapter.
    pub fn new(git: Git) -> Self {
        Self { git }
    }
    /// Create a commit-only reference after explicit caller confirmation of its limited coverage.
    pub fn create_commit_only(
        &self,
        repo: &Path,
    ) -> Result<rit_core::checkpoint::CommitCheckpoint, ApplicationError> {
        Ok(self.git.checkpoint_commit(repo)?)
    }
    /// List commit-only checkpoint references.
    pub fn list(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::checkpoint::CommitCheckpoint>, ApplicationError> {
        Ok(self.git.checkpoint_list(repo)?)
    }
    /// Check reachable commit objects using Git fsck.
    pub fn verify(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
    ) -> Result<rit_core::checkpoint::CommitCheckpoint, ApplicationError> {
        Ok(self.git.checkpoint_verify(repo, id)?)
    }
}
/// Ephemeral activity observations from real Git worktrees.
pub mod activity;

/// Git-ref-backed cooperative workspace service, not authenticated storage or a sandbox.
pub struct WorkspaceService {
    git: Git,
}
impl WorkspaceService {
    /// Construct an independent Git workspace service.
    pub fn new(git: Git) -> Self {
        Self { git }
    }
    /// Create a real linked Git worktree and durable coordination manifest.
    pub fn create(
        &self,
        repo: &Path,
        path: &Path,
        name: &str,
        base: &str,
    ) -> Result<rit_core::workspace::WorkspaceManifest, ApplicationError> {
        Ok(self.git.workspace_create(repo, path, name, base)?)
    }
    /// Reconcile manifest, branch and worktree claims against Git inventory.
    pub fn reconcile(
        &self,
        repo: &Path,
    ) -> Result<rit_core::workspace::WorkspaceReconciliation, ApplicationError> {
        Ok(self.git.workspace_reconcile(repo)?)
    }
    /// Lock or unlock the matched Git worktree.
    pub fn lock(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
        reason: Option<&str>,
    ) -> Result<(), ApplicationError> {
        Ok(self.git.workspace_lock(repo, id, reason)?)
    }
}
/// Native notification watcher; bounded and ephemeral, with reconciliation required.
pub mod watcher;
