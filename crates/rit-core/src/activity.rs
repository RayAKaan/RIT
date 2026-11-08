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
