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

/// One observed change between two Git snapshots, not a complete historical event.
#[derive(Serialize)]
pub struct GitTransition {
    /// Observation category; never infers an actor or a user command.
    pub kind: &'static str,
    /// Git-reported worktree path.
    pub worktree: String,
    /// Repository-relative path for index-status changes.
    pub path: Option<String>,
    /// Old observed value, if any.
    pub before: Option<String>,
    /// New observed value, if any.
    pub after: Option<String>,
    /// This is endpoint comparison, not a complete event history.
    pub evidence: &'static str,
}
/// Bounded endpoint-difference result.
#[derive(Serialize)]
pub struct GitTransitionBatch {
    /// Changes visible between observations.
    pub transitions: Vec<GitTransition>,
    /// True if the 4096-result bound was exceeded.
    pub truncated: bool,
    /// Observations can miss transitions that occurred and reverted between samples.
    pub complete_event_history: bool,
}
/// Compare live Git inventories and status XY columns without claiming complete event history.
pub fn compare_snapshots(
    before: &ActivitySnapshot,
    after: &ActivitySnapshot,
) -> GitTransitionBatch {
    let old = before
        .worktrees
        .iter()
        .map(|w| (w.worktree.path.as_str(), w))
        .collect::<BTreeMap<_, _>>();
    let new = after
        .worktrees
        .iter()
        .map(|w| (w.worktree.path.as_str(), w))
        .collect::<BTreeMap<_, _>>();
    let mut transitions = Vec::new();
    let mut truncated = false;
    let mut push = |kind,
                    worktree: &str,
                    path: Option<String>,
                    before: Option<String>,
                    after: Option<String>| {
        if transitions.len() < 4096 {
            transitions.push(GitTransition {
                kind,
                worktree: worktree.into(),
                path,
                before,
                after,
                evidence: "git_snapshot_endpoints; intermediate changes and actor unknown",
            })
        } else {
            truncated = true
        }
    };
    for (path, now) in &new {
        let Some(was) = old.get(path) else {
            push("worktree_added", path, None, None, Some("present".into()));
            continue;
        };
        if was.worktree.branch != now.worktree.branch {
            push(
                "branch_attachment_changed",
                path,
                None,
                was.worktree.branch.clone(),
                now.worktree.branch.clone(),
            )
        }
        if was.worktree.head != now.worktree.head {
            push(
                "head_commit_changed",
                path,
                None,
                was.worktree.head.as_ref().map(ToString::to_string),
                now.worktree.head.as_ref().map(ToString::to_string),
            )
        }
        if was.worktree.locked != now.worktree.locked {
            push(
                "worktree_lock_changed",
                path,
                None,
                Some(was.worktree.locked.to_string()),
                Some(now.worktree.locked.to_string()),
            )
        }
        if let (Some(a), Some(b)) = (&was.status, &now.status) {
            let index = |s: &Status| {
                s.changes
                    .iter()
                    .filter_map(|c| {
                        c.xy.as_ref()
                            .and_then(|xy| xy.chars().next())
                            .filter(|ch| *ch != '.' && *ch != '?')
                            .map(|ch| (c.path.clone(), ch.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>()
            };
            let x = index(a);
            let y = index(b);
            let paths = x
                .keys()
                .chain(y.keys())
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            for file in paths {
                let prior = x.get(&file);
                let next = y.get(&file);
                if prior != next {
                    push(
                        "index_status_changed",
                        path,
                        Some(file),
                        prior.cloned(),
                        next.cloned(),
                    )
                }
            }
        }
    }
    for path in old.keys().filter(|p| !new.contains_key(*p)) {
        push("worktree_removed", path, None, Some("present".into()), None)
    }
    GitTransitionBatch {
        transitions,
        truncated,
        complete_event_history: false,
    }
}
