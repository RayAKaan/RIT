//! Ephemeral, polling-based Git worktree reconciliation. This is not a filesystem watcher.
use crate::{ApplicationError, GitReadService};
use rit_core::{git_object::WorktreeRecord, repository::Status};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// A worktree plus its current Git status when it can be safely inspected.
#[derive(Serialize)]
pub struct WorktreeActivity {
    /// Git inventory record.
    pub worktree: WorktreeRecord,
    /// Actual Git status, or None for a bare/unavailable entry.
    pub status: Option<Status>,
    /// Status of observation, not a statement about OS isolation.
    pub observation: &'static str,
}
/// One participant in an observed shared-path overlap.
#[derive(Serialize)]
pub struct OverlapParticipant {
    /// Worktree path, not a durable identity.
    pub path: String,
    /// Git branch when attached.
    pub branch: Option<String>,
    /// Git XY status, or None for an untracked file.
    pub xy: Option<String>,
    /// Git change category.
    pub kind: String,
}
/// Exact-path overlap is not proof of a merge conflict.
#[derive(Serialize)]
pub struct ObservedOverlap {
    /// Exact Git-reported repository-relative path.
    pub path: String,
    /// Worktrees reporting this path.
    pub worktrees: Vec<OverlapParticipant>,
}
/// Ephemeral Git reconciliation snapshot, potentially stale immediately after observation.
#[derive(Serialize)]
pub struct ActivitySnapshot {
    /// UTC Unix seconds of observation.
    pub observed_at: u64,
    /// Inventory and status observations.
    pub worktrees: Vec<WorktreeActivity>,
    /// Exact-path overlaps; absence is not proof of no conflict.
    pub overlaps: Vec<ObservedOverlap>,
    /// Always false; polling can miss intermediate changes.
    pub complete_event_history: bool,
}
impl GitReadService {
    /// Re-observe Git worktrees and reconcile their status without persistent storage.
    pub fn activity_snapshot(&self, repo: &Path) -> Result<ActivitySnapshot, ApplicationError> {
        let base = self.inspect(repo)?;
        let inventory = self.worktrees(repo)?;
        if inventory.len() > 64 {
            return Err(ApplicationError::Git(rit_git::GitError::Parse(
                "worktree count exceeds 64",
            )));
        }
        let mut worktrees = Vec::new();
        let mut paths: BTreeMap<String, Vec<OverlapParticipant>> = BTreeMap::new();
        for w in inventory {
            let observed = if w.bare {
                None
            } else {
                self.inspect(Path::new(&w.path))
                    .ok()
                    .filter(|ins| ins.common_git_dir == base.common_git_dir)
            };
            let status = observed.and_then(|ins| ins.status);
            let observation = if w.bare {
                "bare"
            } else if status.is_some() {
                "observed"
            } else {
                "unavailable_or_mismatch"
            };
            if let Some(s) = &status {
                for change in &s.changes {
                    paths
                        .entry(change.path.clone())
                        .or_default()
                        .push(OverlapParticipant {
                            path: w.path.clone(),
                            branch: w.branch.clone(),
                            xy: change.xy.clone(),
                            kind: change.kind.clone(),
                        });
                }
            }
            worktrees.push(WorktreeActivity {
                worktree: w,
                status,
                observation,
            });
        }
        let overlaps = paths
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(path, worktrees)| ObservedOverlap { path, worktrees })
            .collect();
        let observed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                ApplicationError::Git(rit_git::GitError::Parse("system clock before Unix epoch"))
            })?
            .as_secs();
        Ok(ActivitySnapshot {
            observed_at,
            worktrees,
            overlaps,
            complete_event_history: false,
        })
    }
}
