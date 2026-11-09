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

/// One immutable object stage reported by Git's in-memory merge engine.
#[derive(Clone, Debug, Serialize)]
pub struct MergeStage {
    /// Git conflict path.
    pub path: String,
    /// Base=1, left=2, right=3.
    pub stage: u8,
    /// Git tree entry mode.
    pub mode: String,
    /// Stage object, which may be a submodule commit rather than a blob.
    pub object: crate::git_object::ObjectId,
}
/// One Git-provided merge classification and related paths.
#[derive(Clone, Debug, Serialize)]
pub struct MergeMessage {
    /// Paths named by Git for this diagnostic.
    pub paths: Vec<String>,
    /// Machine-oriented Git merge message type.
    pub kind: String,
    /// Git's text, potentially containing repository path names.
    pub detail: String,
}
/// Ephemeral Git merge simulation; result tree lives only in an isolated temporary object store.
#[derive(Clone, Debug, Serialize)]
pub struct MergeSimulation {
    /// Exact left commit.
    pub left: crate::git_object::ObjectId,
    /// Exact right commit.
    pub right: crate::git_object::ObjectId,
    /// All Git-reported best merge bases.
    pub merge_bases: Vec<crate::git_object::ObjectId>,
    /// First best merge base used only for changed-path summaries.
    pub comparison_base: crate::git_object::ObjectId,
    /// Paths changed from comparison base to left (no rename inference).
    pub changed_left: Vec<String>,
    /// Paths changed from comparison base to right (no rename inference).
    pub changed_right: Vec<String>,
    /// Resulting temporary tree ID; not retained after operation completes.
    pub ephemeral_tree: crate::git_object::ObjectId,
    /// Whether Git returned a conflict status.
    pub has_conflicts: bool,
    /// Git-reported unmerged stage evidence.
    pub stages: Vec<MergeStage>,
    /// Git-provided path and conflict classifications.
    pub messages: Vec<MergeMessage>,
    /// Always true; this does not authorize or apply a resolution.
    pub human_review_required: bool,
}
