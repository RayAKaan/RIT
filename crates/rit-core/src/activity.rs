//! Privacy-preserving Git observations; never keystrokes or process surveillance.
use crate::{
    repository::{RepositoryId, Status},
    workspace::WorkspaceId,
};
use serde::{Deserialize, Serialize};
/// A point-in-time Git observation, not a live or complete activity feed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// Source repository.
    pub repository: RepositoryId,
    /// Optional associated workspace.
    pub workspace: Option<WorkspaceId>,
    /// Caller-supplied Unix timestamp in seconds.
    pub observed_at: u64,
    /// Git snapshot.
    pub status: Status,
}
/// Difference between snapshots. This is not a history of events between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciliation {
    /// Branch or commit changed.
    pub head_changed: bool,
    /// Changed or newly reported paths.
    pub changed_paths: Vec<String>,
    /// Paths no longer reported as changed.
    pub cleared_paths: Vec<String>,
}
impl Observation {
    /// True if time is in the future or exceeds the allowed age.
    pub fn is_stale(&self, now: u64, max_age: u64) -> bool {
        now.checked_sub(self.observed_at)
            .is_none_or(|age| age > max_age)
    }
    /// Compare two observations from the same repository and workspace.
    pub fn reconcile(&self, new: &Self) -> Result<Reconciliation, &'static str> {
        if self.repository != new.repository || self.workspace != new.workspace {
            return Err("observation source mismatch");
        }
        let old: std::collections::BTreeMap<_, _> =
            self.status.changes.iter().map(|c| (&c.path, c)).collect();
        let current: std::collections::BTreeMap<_, _> =
            new.status.changes.iter().map(|c| (&c.path, c)).collect();
        Ok(Reconciliation {
            head_changed: self.status.branch != new.status.branch
                || self.status.oid != new.status.oid,
            changed_paths: current
                .iter()
                .filter(|(p, c)| old.get(*p) != Some(*c))
                .map(|(p, _)| (*p).clone())
                .collect(),
            cleared_paths: old
                .keys()
                .filter(|p| !current.contains_key(*p))
                .map(|p| (*p).clone())
                .collect(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Change;
    #[test]
    fn reconciliation_and_staleness() {
        let status = Status {
            branch: Some("main".into()),
            oid: Some("old".into()),
            upstream: None,
            ahead: None,
            behind: None,
            changes: vec![],
        };
        let old = Observation {
            repository: RepositoryId::new(),
            workspace: None,
            observed_at: 10,
            status,
        };
        let mut new = old.clone();
        new.status.oid = Some("new".into());
        new.status.changes.push(Change {
            kind: "untracked".into(),
            path: "file".into(),
            original_path: None,
            xy: None,
            submodule: None,
        });
        let d = old.reconcile(&new).unwrap();
        assert!(d.head_changed);
        assert_eq!(d.changed_paths, vec!["file"]);
        assert!(old.is_stale(9, 30));
        assert!(old.is_stale(41, 30));
        assert!(!old.is_stale(40, 30));
    }
}

/// An exact-path change from one workspace snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceChange {
    /// Workspace reporting this path.
    pub workspace: WorkspaceId,
    /// Git record kind, including untracked and unmerged.
    pub kind: String,
    /// Git index reports a change; unavailable for records without XY.
    pub staged: Option<bool>,
    /// Git working-tree column reports a change; unavailable for records without XY.
    pub unstaged: Option<bool>,
    /// This workspace snapshot is stale relative to the caller's clock and maximum age.
    pub stale: bool,
}
/// Multiple snapshots reporting the same Git path. This is not proof of a merge conflict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOverlap {
    /// Exact path from the supplied Git status records, not case-folded or canonicalized.
    pub path: String,
    /// Workspaces observed reporting the path.
    pub workspaces: Vec<WorkspaceChange>,
}
/// Compare current snapshots for exact-path overlaps without accessing files or writing storage.
/// Inputs must cover one repository and have distinct, explicit workspace IDs. A missing overlap
/// is not evidence that a conflict cannot occur, and rename sources are not matched.
pub fn overlaps(
    observations: &[Observation],
    now: u64,
    max_age: u64,
) -> Result<Vec<FileOverlap>, &'static str> {
    use std::collections::{BTreeMap, HashSet};
    let mut seen = HashSet::new();
    let mut repo = None;
    let mut paths: BTreeMap<&str, Vec<WorkspaceChange>> = BTreeMap::new();
    for observation in observations {
        let id = observation.workspace.ok_or("workspace ID required")?;
        if !seen.insert(id) {
            return Err("duplicate workspace snapshot");
        }
        if let Some(r) = repo {
            if r != observation.repository {
                return Err("repository mismatch");
            }
        } else {
            repo = Some(observation.repository)
        }
        let mut seen_paths = HashSet::new();
        for change in &observation.status.changes {
            if !seen_paths.insert(&change.path) {
                return Err("duplicate path in snapshot");
            }
            let flags = change.xy.as_deref().and_then(|xy| {
                let b = xy.as_bytes();
                if b.len() == 2 {
                    Some((b[0] != b'.', b[1] != b'.'))
                } else {
                    None
                }
            });
            paths
                .entry(&change.path)
                .or_default()
                .push(WorkspaceChange {
                    workspace: id,
                    kind: change.kind.clone(),
                    staged: flags.map(|v| v.0),
                    unstaged: flags.map(|v| v.1),
                    stale: observation.is_stale(now, max_age),
                });
        }
    }
    Ok(paths
        .into_iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|(path, workspaces)| FileOverlap {
            path: path.to_owned(),
            workspaces,
        })
        .collect())
}
