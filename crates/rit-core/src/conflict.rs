//! Conflict evidence, never an automatic resolution.
use crate::repository::Status;
use serde::{Deserialize, Serialize};
/// Supported adapter targets. No semantic adapters are implemented yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    /// TypeScript.
    TypeScript,
    /// Python.
    Python,
    /// Go.
    Go,
    /// Other or unknown.
    Unknown,
}
/// A conflict detected by Git status, without claims about semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictEvidence {
    /// Path as supplied by Git.
    pub path: String,
    /// Raw status code when available.
    pub git_xy: Option<String>,
    /// Human inspection is required.
    pub human_review_required: bool,
}
/// Extract Git's unmerged entries from a snapshot; does not inspect contents.
pub fn git_unmerged(status: &Status) -> Vec<ConflictEvidence> {
    status
        .changes
        .iter()
        .filter(|c| c.kind == "unmerged")
        .map(|c| ConflictEvidence {
            path: c.path.clone(),
            git_xy: c.xy.clone(),
            human_review_required: true,
        })
        .collect()
}
/// A proposed semantic result must carry evidence; it is not automatically trusted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticEvidence {
    /// Language used.
    pub language: Language,
    /// Provider and version.
    pub provider: String,
    /// Source object or digest identifiers.
    pub inputs: Vec<String>,
    /// Result explanation; human validation is mandatory.
    pub explanation: String,
    /// Provider-supplied confidence, not a safety guarantee.
    pub confidence: Option<f32>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Change;
    #[test]
    fn extracts_only_unmerged() {
        let s = Status {
            branch: None,
            oid: None,
            upstream: None,
            ahead: None,
            behind: None,
            changes: vec![
                Change {
                    kind: "unmerged".into(),
                    path: "file".into(),
                    original_path: None,
                    xy: Some("UU".into()),
                    submodule: None,
                },
                Change {
                    kind: "untracked".into(),
                    path: "other".into(),
                    original_path: None,
                    xy: None,
                    submodule: None,
                },
            ],
        };
        let e = git_unmerged(&s);
        assert_eq!(e.len(), 1);
        assert!(e[0].human_review_required);
    }
}
