//! Conservative graph traversal over caller-supplied dependency evidence.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
/// One evidenced dependency: dependent uses dependency. The edge itself is not validated here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    /// Dependent module or file identifier.
    pub dependent: String,
    /// Dependency module or file identifier.
    pub dependency: String,
    /// Provenance supplied by the extractor.
    pub evidence: String,
}
/// Explicitly partial graph; no completeness is inferred from traversing it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// Commit or snapshot identifier used by extractor.
    pub source_revision: String,
    /// Evidence-backed edges supplied by caller.
    pub edges: Vec<DependencyEdge>,
}
/// Conservative reachability within the supplied graph, not all real-world impact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpactReport {
    /// Affected dependent identifiers in stable order, excluding the seed itself.
    pub known_dependents: Vec<String>,
    /// True when the graph revision differs from requested revision.
    pub stale: bool,
    /// Always false because completeness is unknown.
    pub complete: bool,
}
impl DependencyGraph {
    /// Reverse transitively traverse supplied edges, safely terminating on cycles.
    pub fn impact(&self, changed: &str, current_revision: &str) -> ImpactReport {
        let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for e in &self.edges {
            reverse.entry(&e.dependency).or_default().push(&e.dependent);
        }
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::new();
        seen.insert(changed);
        q.push_back(changed);
        while let Some(node) = q.pop_front() {
            if let Some(parents) = reverse.get(node) {
                for parent in parents {
                    if seen.insert(parent) {
                        q.push_back(parent);
                    }
                }
            }
        }
        seen.remove(changed);
        ImpactReport {
            known_dependents: seen.into_iter().map(str::to_owned).collect(),
            stale: self.source_revision != current_revision,
            complete: false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traverses_cycle_and_reports_stale() {
        let edge = |a: &str, b: &str| DependencyEdge {
            dependent: a.into(),
            dependency: b.into(),
            evidence: "test".into(),
        };
        let g = DependencyGraph {
            source_revision: "old".into(),
            edges: vec![edge("b", "a"), edge("c", "b"), edge("a", "c")],
        };
        let r = g.impact("a", "new");
        assert_eq!(r.known_dependents, vec!["b", "c"]);
        assert!(r.stale);
        assert!(!r.complete);
    }
}
