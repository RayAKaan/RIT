//! Application services coordinating independent Git and registry adapters.
#![forbid(unsafe_code)]
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
        if data.starts_with(&ins.location)
            || data.starts_with(&ins.common_git_dir)
            || data.starts_with(&ins.git_dir)
        {
            return Err(ApplicationError::DataInsideRepository);
        }
        Ok(Registry::open(&self.data_dir)?.register(&ins, name)?)
    }
    /// List records without invoking Git.
    pub fn list(&self) -> Result<Vec<Repository>, ApplicationError> {
        match Registry::open_readonly(&self.data_dir)? {
            Some(registry) => Ok(registry.list()?),
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
        let path = if selector.parse::<RepositoryId>().is_ok() {
            r.locations
                .iter()
                .find(|l| Path::new(&l.path).is_dir())
                .map(|l| PathBuf::from(&l.path))
                .ok_or(ApplicationError::StaleLocation)?
        } else {
            PathBuf::from(selector)
        };
        let ins = self.git.inspect(&path)?;
        if ins.common_git_dir != r.common_git_dir {
            return Err(ApplicationError::StaleLocation);
        }
        Ok((r, ins))
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
