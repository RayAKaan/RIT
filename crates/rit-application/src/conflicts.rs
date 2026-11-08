//! Real temporary-object Git merge simulation and advisory syntax evidence.
use crate::ApplicationError;
use rit_core::conflict::{Language, MergeSimulation};
use rit_git::Git;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Git-native classification, not proof of semantic safety.
#[derive(Serialize)]
pub struct Classification {
    /// Git-reported paths.
    pub paths: Vec<String>,
    /// Conservative classification.
    pub category: &'static str,
    /// Git's machine-oriented diagnostic type.
    pub evidence: String,
}
/// Candidate declaration changed on both sides, requiring human interpretation.
#[derive(Serialize)]
pub struct StructuralCandidate {
    /// Conflicted path.
    pub path: String,
    /// Tree-sitter declaration kind.
    pub kind: String,
    /// Declaration name.
    pub name: String,
    /// Base syntax-node hash if available.
    pub base_sha256: Option<String>,
    /// Left and right syntax-node hashes.
    pub left_sha256: String,
    /// Right syntax-node hash.
    pub right_sha256: String,
    /// Analysis is advisory, not a semantic verdict.
    pub confidence: &'static str,
}
/// Ephemeral merge report.
#[derive(Serialize)]
pub struct ConflictReport {
    /// Git stage/blob evidence and merge-base identity.
    pub simulation: MergeSimulation,
    /// Classifications derived from Git messages, plus binary/submodule evidence.
    pub classifications: Vec<Classification>,
    /// Both-side declaration changes, not automatic resolution.
    pub structural_candidates: Vec<StructuralCandidate>,
    /// Files with unsupported, missing, invalid or oversized structural inputs.
    pub structural_unavailable: Vec<String>,
    /// No semantic completeness or resolution verification is claimed.
    pub complete: bool,
}
/// Application service shared by interfaces; never publishes or applies a merge.
pub struct ConflictService {
    git: Git,
}
impl ConflictService {
    /// Construct an isolated simulation service.
    pub fn new(git: Git) -> Self {
        Self { git }
    }
    /// Run Git's real merge engine in a temporary object store, then inspect conflict blobs.
    pub fn simulate(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
    ) -> Result<ConflictReport, ApplicationError> {
        let simulation = self.git.simulate_merge(repo, left, right)?;
        let mut classes = Vec::new();
        for msg in &simulation.messages {
            let k = msg.kind.to_ascii_lowercase();
            let cat = if k.contains("rename") {
                "rename_conflict"
            } else if k.contains("modify/delete") || k.contains("delete/modify") {
                "delete_modify_conflict"
            } else if k.contains("add/add") {
                "add_add_conflict"
            } else if k.contains("file/directory") {
                "file_directory_conflict"
            } else if k.contains("binary") {
                "binary_conflict"
            } else if k.contains("submodule") {
                "submodule_conflict"
            } else if k.contains("conflict") {
                "content_or_other_conflict"
            } else {
                continue;
            };
            classes.push(Classification {
                paths: msg.paths.clone(),
                category: cat,
                evidence: msg.kind.clone(),
            });
        }
        let mut grouped: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for stage in &simulation.stages {
            grouped.entry(stage.path.clone()).or_default().push(stage)
        }
        let mut structural = Vec::new();
        let mut unavailable = BTreeSet::new();
        for (path, stages) in grouped {
            if stages.iter().any(|s| s.mode == "160000") {
                classes.push(Classification {
                    paths: vec![path.clone()],
                    category: "submodule_conflict",
                    evidence: "gitlink stage".into(),
                });
                continue;
            }
            let language = if path.ends_with(".py") {
                Language::Python
            } else if path.ends_with(".ts") {
                Language::TypeScript
            } else if path.ends_with(".go") {
                Language::Go
            } else {
                Language::Unknown
            };
            let mut parsed: BTreeMap<u8, Vec<rit_analysis::DeclarationObservation>> =
                BTreeMap::new();
            let mut binary = false;
            for s in stages {
                if !matches!(s.mode.as_str(), "100644" | "100755") {
                    unavailable.insert(path.clone());
                    continue;
                }
                let bytes = match self.git.conflict_blob(repo, &s.object) {
                    Ok(v) => v,
                    Err(_) => {
                        unavailable.insert(path.clone());
                        continue;
                    }
                };
                if bytes.contains(&0) {
                    binary = true;
                    continue;
                }
                if language == Language::Unknown {
                    continue;
                }
                match rit_analysis::declarations(language, &bytes) {
                    Ok(v) => {
                        parsed.insert(s.stage, v);
                    }
                    Err(_) => {
                        unavailable.insert(path.clone());
                    }
                }
            }
            if binary {
                classes.push(Classification {
                    paths: vec![path.clone()],
                    category: "binary_conflict",
                    evidence: "NUL byte in conflict stage blob".into(),
                });
                continue;
            }
            if let (Some(left), Some(right)) = (parsed.get(&2), parsed.get(&3)) {
                for l in left {
                    let matching = right
                        .iter()
                        .filter(|r| r.kind == l.kind && r.name == l.name)
                        .collect::<Vec<_>>();
                    let unique_left = left
                        .iter()
                        .filter(|x| x.kind == l.kind && x.name == l.name)
                        .count()
                        == 1;
                    if matching.len() != 1 || !unique_left {
                        continue;
                    }
                    let rr = matching[0];
                    if l.syntax_sha256 == rr.syntax_sha256 {
                        continue;
                    }
                    let base = parsed.get(&1).and_then(|v| {
                        let m = v
                            .iter()
                            .filter(|x| x.kind == l.kind && x.name == l.name)
                            .collect::<Vec<_>>();
                        if m.len() == 1 {
                            Some(m[0].syntax_sha256.clone())
                        } else {
                            None
                        }
                    });
                    if base
                        .as_ref()
                        .is_some_and(|b| b == &l.syntax_sha256 || b == &rr.syntax_sha256)
                    {
                        continue;
                    }
                    structural.push(StructuralCandidate {
                        path: path.clone(),
                        kind: l.kind.clone(),
                        name: l.name.clone(),
                        base_sha256: base,
                        left_sha256: l.syntax_sha256.clone(),
                        right_sha256: rr.syntax_sha256.clone(),
                        confidence: "advisory_both_sides_changed_not_semantic_proof",
                    });
                }
            }
        }
        Ok(ConflictReport {
            simulation,
            classifications: classes,
            structural_candidates: structural,
            structural_unavailable: unavailable.into_iter().collect(),
            complete: false,
        })
    }
}
