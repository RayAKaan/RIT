//! Bounded, read-only syntax extraction over explicit source bytes.
//! No filesystem access, authentication, import resolution or complete semantics are implied.
#![forbid(unsafe_code)]
use rit_core::conflict::Language;
use serde::Serialize;
use thiserror::Error;
use tree_sitter::{Node, Parser};

/// Syntax parser result is intentionally partial, even when parsing succeeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportObservation {
    /// Raw static import statement as written; may contain aliases or multiline syntax.
    pub source: String,
    /// One-based source line.
    pub line: usize,
    /// Tree-sitter node kind, useful for auditing the extraction.
    pub node_kind: String,
    /// Conservatively extracted literal module specifiers; unresolved strings are not dependencies.
    pub specifiers: Vec<String>,
}
/// Syntax extraction with explicit parse/coverage warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// Static import observations, in source order.
    pub imports: Vec<ImportObservation>,
    /// Whether Tree-sitter reported syntax errors or missing nodes.
    pub syntax_errors: bool,
    /// Always false: dynamic imports, resolution and runtime dependencies are not complete.
    pub complete: bool,
}
/// Errors never contain source content.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AnalysisError {
    /// Language unavailable.
    #[error("unsupported language")]
    UnsupportedLanguage,
    /// Input is too large for this local parser.
    #[error("source exceeds analysis limit")]
    InputLimit,
    /// Invalid UTF-8 source is not analyzed.
    #[error("source is not valid UTF-8")]
    InvalidUtf8,
    /// Grammar could not be loaded.
    #[error("parser language initialization failed")]
    Grammar,
    /// Tree-sitter did not return a tree.
    #[error("syntax parser did not produce a tree")]
    Parse,
    /// Too many syntax nodes or import records.
    #[error("syntax tree exceeds analysis limit")]
    ComplexityLimit,
}
const MAX_BYTES: usize = 1_048_576;
const MAX_NODES: usize = 100_000;
const MAX_IMPORTS: usize = 10_000;
/// Extract explicit import syntax from caller-provided UTF-8 source only.
/// The caller must provide authorization and associate the report with an actual revision.
/// The returned strings are source content and must be handled as sensitive data.
pub fn imports(language: Language, source: &[u8]) -> Result<ImportReport, AnalysisError> {
    if source.len() > MAX_BYTES {
        return Err(AnalysisError::InputLimit);
    }
    std::str::from_utf8(source).map_err(|_| AnalysisError::InvalidUtf8)?;
    let grammar = match language {
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Unknown => return Err(AnalysisError::UnsupportedLanguage),
    };
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|_| AnalysisError::Grammar)?;
    let tree = parser.parse(source, None).ok_or(AnalysisError::Parse)?;
    let root = tree.root_node();
    let mut out = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > MAX_NODES {
            return Err(AnalysisError::ComplexityLimit);
        }
        if is_import(language, node) {
            if out.len() >= MAX_IMPORTS {
                return Err(AnalysisError::ComplexityLimit);
            }
            let text = node
                .utf8_text(source)
                .map_err(|_| AnalysisError::InvalidUtf8)?;
            out.push(ImportObservation {
                source: text.to_owned(),
                line: node.start_position().row + 1,
                node_kind: node.kind().to_owned(),
                specifiers: specifiers(language, node, source),
            });
            // Do not duplicate nested Python import nodes within an import statement.
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(ImportReport {
        imports: out,
        syntax_errors: root.has_error(),
        complete: false,
    })
}
fn specifiers(language: Language, node: Node<'_>, source: &[u8]) -> Vec<String> {
    fn text(node: Node<'_>, src: &[u8]) -> Option<String> {
        node.utf8_text(src).ok().map(str::to_owned)
    }
    fn literal(node: Node<'_>, src: &[u8]) -> Option<String> {
        let s = node.utf8_text(src).ok()?;
        let quote = s.as_bytes().first()?;
        if !matches!(quote, b'\'' | b'"' | b'`')
            || s.as_bytes().last() != Some(quote)
            || s.len() < 2
        {
            return None;
        }
        let inner = &s[1..s.len() - 1];
        if inner.contains('\\') {
            return None;
        }
        Some(inner.to_owned())
    }
    match language {
        Language::Python if node.kind() == "import_from_statement" => node
            .child_by_field_name("module_name")
            .and_then(|v| text(v, source))
            .into_iter()
            .collect(),
        Language::Python => {
            let mut cursor = node.walk();
            node.children_by_field_name("name", &mut cursor)
                .filter_map(|n| {
                    if n.kind() == "aliased_import" {
                        n.child_by_field_name("name").and_then(|x| text(x, source))
                    } else {
                        text(n, source)
                    }
                })
                .collect()
        }
        Language::TypeScript => node
            .child_by_field_name("source")
            .and_then(|v| literal(v, source))
            .into_iter()
            .collect(),
        Language::Go => {
            let mut found = Vec::new();
            let mut stack = vec![node];
            while let Some(n) = stack.pop() {
                if n.kind() == "import_spec" {
                    if let Some(v) = n
                        .child_by_field_name("path")
                        .and_then(|v| literal(v, source))
                    {
                        found.push(v)
                    }
                } else {
                    let mut cursor = n.walk();
                    let children: Vec<_> = n.named_children(&mut cursor).collect();
                    stack.extend(children.into_iter().rev());
                }
            }
            found
        }
        Language::Unknown => Vec::new(),
    }
}
fn is_import(language: Language, node: Node<'_>) -> bool {
    match language {
        Language::Python => matches!(node.kind(), "import_statement" | "import_from_statement"),
        Language::TypeScript => {
            node.kind() == "import_statement"
                || (node.kind() == "export_statement"
                    && node.child_by_field_name("source").is_some())
        }
        Language::Go => node.kind() == "import_declaration",
        Language::Unknown => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_real_static_syntax_without_treating_comments_or_strings_as_imports() {
        let cases = [
            (
                Language::Python,
                "# import bogus\ns = 'import nope'\nimport os\nfrom pkg import item\n",
                2,
            ),
            (
                Language::TypeScript,
                "// import 'bogus';\nconst s = \"import nope\";\nimport {x} from './x';\nexport * from './y';\n",
                2,
            ),
            (
                Language::Go,
                "package x\n// import bogus\nimport (\n \"fmt\"\n \"example.org/a\"\n)\n",
                1,
            ),
        ];
        for (lang, source, count) in cases {
            let report = imports(lang, source.as_bytes()).unwrap();
            assert_eq!(report.imports.len(), count, "{lang:?}: {report:?}");
            assert!(!report.syntax_errors);
            assert!(!report.complete);
        }
    }
    #[test]
    fn errors_and_partial_parse_are_explicit() {
        assert_eq!(
            imports(Language::Unknown, b"x"),
            Err(AnalysisError::UnsupportedLanguage)
        );
        assert_eq!(
            imports(Language::Python, &[0xff]),
            Err(AnalysisError::InvalidUtf8)
        );
        assert_eq!(
            imports(Language::Go, &vec![b'x'; MAX_BYTES + 1]),
            Err(AnalysisError::InputLimit)
        );
        let result = imports(Language::Python, b"def f(:\n import os\n").unwrap();
        assert!(result.syntax_errors);
        assert!(!result.complete);
    }
}

/// One candidate internal dependency inferred from a literal import in a committed blob.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedImport {
    /// Importing path.
    pub dependent: String,
    /// Candidate imported path; resolution is heuristic, not runtime proof.
    pub dependency: String,
    /// One-based import statement line.
    pub line: usize,
    /// Literal import specifier.
    pub specifier: String,
    /// Resolution confidence.
    pub confidence: &'static str,
    /// Resolution algorithm, not a runtime guarantee.
    pub resolution_method: &'static str,
    /// One-based source position and parser provenance.
    pub parser_version: &'static str,
    /// SHA-256 of importing source bytes.
    pub source_sha256: String,
    /// Source commit, or base commit when worktree bytes are analyzed.
    pub snapshot_commit: String,
    /// Exact content observation ID (non-atomic for worktree sources).
    pub snapshot_id: String,
    /// Directed relationship type.
    pub edge_type: &'static str,
}
/// An import that was not resolved unambiguously within the selected source tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnresolvedImport {
    /// Importing path.
    pub dependent: String,
    /// One-based import line.
    pub line: usize,
    /// Literal specifier.
    pub specifier: String,
    /// Explanation; not a claim of an error in the source.
    pub reason: &'static str,
}
/// Evidence for a source file or module manifest.
#[derive(Serialize)]
pub struct GraphNode {
    /// Repository relative path.
    pub path: String,
    /// Source language or module manifest.
    pub node_type: &'static str,
    /// SHA-256 of analyzed bytes.
    pub source_sha256: String,
    /// Grammar version where applicable.
    pub parser_version: &'static str,
}
/// Explicitly incomplete repository snapshot analysis.
#[derive(Serialize)]
pub struct RepositoryGraph {
    /// Exact immutable source commit.
    pub commit: rit_core::git_object::ObjectId,
    /// Content snapshot identifier, not a durable authenticated record.
    pub snapshot_id: String,
    /// Worktree observations are not atomic.
    pub non_atomic: bool,
    /// Supported files analyzed.
    pub files: Vec<String>,
    /// Analyzed file evidence.
    pub nodes: Vec<GraphNode>,
    /// Heuristic internal dependencies.
    pub edges: Vec<ResolvedImport>,
    /// Imports not resolved by these limited rules.
    pub unresolved: Vec<UnresolvedImport>,
    /// Files with syntax errors or invalid UTF-8.
    pub problematic_files: Vec<String>,
    /// Source-like symlinks and submodules omitted by Git adapter.
    pub omitted_non_regular: usize,
    /// Cycles in the observed heuristic graph, as strongly connected components.
    pub cycles: Vec<Vec<String>>,
    /// Always false: this analysis is not comprehensive.
    pub complete: bool,
    /// Working tree changes are not included.
    pub scope: &'static str,
}
/// Analyze tracked source blobs obtained by the Git adapter; never reads the working tree.
pub fn repository_graph(
    snapshot: rit_core::git_object::SourceSnapshot,
) -> Result<RepositoryGraph, AnalysisError> {
    use std::collections::HashSet;
    let names: HashSet<_> = snapshot.files.iter().map(|f| f.path.as_str()).collect();
    use sha2::{Digest, Sha256};
    let modules = go_modules(&snapshot.files);
    let nodes = snapshot
        .files
        .iter()
        .map(|f| {
            let (node_type, parser_version) = if f.path.ends_with(".py") {
                ("python", "tree-sitter-python 0.25.0")
            } else if f.path.ends_with(".ts") {
                ("typescript", "tree-sitter-typescript 0.23.2")
            } else if f.path.ends_with(".go") {
                ("go", "tree-sitter-go 0.25.0")
            } else {
                ("go_module", "line-based go.mod module directive")
            };
            GraphNode {
                path: f.path.clone(),
                node_type,
                source_sha256: format!("{:x}", Sha256::digest(&f.bytes)),
                parser_version,
            }
        })
        .collect();
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    let mut problematic = Vec::new();
    for file in &snapshot.files {
        let language = if file.path.ends_with(".py") {
            Language::Python
        } else if file.path.ends_with(".ts") {
            Language::TypeScript
        } else if file.path.ends_with(".go") {
            Language::Go
        } else {
            continue;
        };
        let report = match imports(language, &file.bytes) {
            Ok(v) => v,
            Err(AnalysisError::InvalidUtf8) => {
                problematic.push(file.path.clone());
                continue;
            }
            Err(e) => return Err(e),
        };
        if report.syntax_errors {
            problematic.push(file.path.clone());
        }
        for observation in report.imports {
            if observation.specifiers.is_empty() {
                if unresolved.len() >= 50_000 {
                    return Err(AnalysisError::ComplexityLimit);
                }
                unresolved.push(UnresolvedImport {
                    dependent: file.path.clone(),
                    line: observation.line,
                    specifier: String::new(),
                    reason: "specifier not extracted",
                });
            }
            for spec in observation.specifiers {
                let candidates = if language == Language::Go {
                    resolve_go(&file.path, &spec, &modules, &names)
                } else {
                    resolve(language, &file.path, &spec, &names)
                };
                if candidates.len() == 1 || (language == Language::Go && !candidates.is_empty()) {
                    for candidate in candidates {
                        if edges.len() >= 50_000 {
                            return Err(AnalysisError::ComplexityLimit);
                        }
                        edges.push(ResolvedImport {
                            dependent: file.path.clone(),
                            dependency: candidate,
                            line: observation.line,
                            specifier: spec.clone(),
                            confidence: "heuristic",
                            resolution_method: if language == Language::Go {
                                "go_mod_local_package_files"
                            } else if language == Language::Python {
                                "python_path_candidates"
                            } else {
                                "typescript_relative_candidates"
                            },
                            parser_version: if language == Language::Go {
                                "tree-sitter-go 0.25.0"
                            } else if language == Language::Python {
                                "tree-sitter-python 0.25.0"
                            } else {
                                "tree-sitter-typescript 0.23.2"
                            },
                            source_sha256: format!("{:x}", Sha256::digest(&file.bytes)),
                            snapshot_commit: snapshot.commit.to_string(),
                            snapshot_id: snapshot.snapshot_id.clone(),
                            edge_type: "static_import",
                        });
                    }
                } else {
                    if unresolved.len() >= 50_000 {
                        return Err(AnalysisError::ComplexityLimit);
                    }
                    unresolved.push(UnresolvedImport {
                        dependent: file.path.clone(),
                        line: observation.line,
                        specifier: spec,
                        reason: if candidates.is_empty() {
                            "unresolved or external"
                        } else {
                            "ambiguous"
                        },
                    });
                }
            }
        }
    }
    let cycles = graph_cycles(&edges);
    Ok(RepositoryGraph {
        commit: snapshot.commit,
        snapshot_id: snapshot.snapshot_id,
        non_atomic: snapshot.non_atomic,
        nodes,
        cycles,
        files: snapshot.files.into_iter().map(|f| f.path).collect(),
        edges,
        unresolved,
        problematic_files: problematic,
        omitted_non_regular: snapshot.omitted_non_regular,
        complete: false,
        scope: snapshot.scope,
    })
}
fn resolve(
    language: Language,
    path: &str,
    spec: &str,
    names: &std::collections::HashSet<&str>,
) -> Vec<String> {
    let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
    let base = match language {
        Language::TypeScript if spec.starts_with("./") || spec.starts_with("../") => {
            let mut components: Vec<&str> = parent.split('/').filter(|x| !x.is_empty()).collect();
            for part in spec.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        if components.pop().is_none() {
                            return Vec::new();
                        }
                    }
                    v if !v.contains('\\') => components.push(v),
                    _ => return Vec::new(),
                }
            }
            components.join("/")
        }
        Language::Python => {
            let dots = spec.bytes().take_while(|b| *b == b'.').count();
            let rest = &spec[dots..];
            if rest.is_empty()
                || !rest.split('.').all(|x| {
                    !x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            {
                return Vec::new();
            }
            let mut components: Vec<&str> = if dots == 0 {
                Vec::new()
            } else {
                parent.split('/').filter(|x| !x.is_empty()).collect()
            };
            for _ in 1..dots {
                if components.pop().is_none() {
                    return Vec::new();
                }
            }
            components.extend(rest.split('.'));
            components.join("/")
        }
        _ => return Vec::new(),
    };
    let candidates = match language {
        Language::Python => vec![format!("{base}.py"), format!("{base}/__init__.py")],
        Language::TypeScript => {
            if base.ends_with(".ts") {
                vec![base]
            } else {
                vec![format!("{base}.ts"), format!("{base}/index.ts")]
            }
        }
        _ => Vec::new(),
    };
    candidates
        .into_iter()
        .filter(|x| names.contains(x.as_str()))
        .collect()
}

fn go_modules(files: &[rit_core::git_object::SourceFile]) -> Vec<(String, String)> {
    let mut modules = Vec::new();
    for f in files {
        if !(f.path == "go.mod" || f.path.ends_with("/go.mod")) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&f.bytes) else {
            continue;
        };
        let names = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("module "))
            .map(|v| v.split_whitespace().next().unwrap_or("").trim_matches('"'))
            .filter(|v| !v.is_empty() && !v.contains('\\'))
            .collect::<Vec<_>>();
        if names.len() == 1 {
            modules.push((
                f.path
                    .strip_suffix("go.mod")
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_owned(),
                names[0].to_owned(),
            ));
        }
    }
    modules
}
fn resolve_go(
    path: &str,
    spec: &str,
    modules: &[(String, String)],
    names: &std::collections::HashSet<&str>,
) -> Vec<String> {
    let Some((dir, module)) = modules
        .iter()
        .filter(|(dir, _)| dir.is_empty() || path.starts_with(&format!("{dir}/")))
        .max_by_key(|(dir, _)| dir.len())
    else {
        return Vec::new();
    };
    let suffix = if spec == module {
        ""
    } else if let Some(s) = spec.strip_prefix(module).and_then(|s| s.strip_prefix('/')) {
        s
    } else {
        return Vec::new();
    };
    if suffix
        .split('/')
        .any(|p| p == ".." || p == "." || p.contains('\\'))
    {
        return Vec::new();
    }
    let package = [dir.as_str(), suffix]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    // Do not cross nested module boundaries, and do not guess build-tag selection.
    if modules.iter().any(|(nested, _)| {
        nested != dir
            && !nested.is_empty()
            && (package == *nested || package.starts_with(&format!("{nested}/")))
    }) {
        return Vec::new();
    }
    let mut files = names
        .iter()
        .filter(|p| p.ends_with(".go") && p.rsplit_once('/').map_or("", |(d, _)| d) == package)
        .map(|p| (*p).to_owned())
        .collect::<Vec<_>>();
    files.sort();
    files
}
fn graph_cycles(edges: &[ResolvedImport]) -> Vec<Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rev: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in edges {
        adj.entry(e.dependent.clone())
            .or_default()
            .insert(e.dependency.clone());
        adj.entry(e.dependency.clone()).or_default();
        rev.entry(e.dependency.clone())
            .or_default()
            .insert(e.dependent.clone());
        rev.entry(e.dependent.clone()).or_default();
    }
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for root in adj.keys() {
        let mut stack = vec![(root.clone(), false)];
        while let Some((n, expanded)) = stack.pop() {
            if expanded {
                order.push(n);
                continue;
            }
            if !seen.insert(n.clone()) {
                continue;
            }
            stack.push((n.clone(), true));
            if let Some(next) = adj.get(&n) {
                for v in next.iter().rev() {
                    if !seen.contains(v) {
                        stack.push((v.clone(), false));
                    }
                }
            }
        }
    }
    seen.clear();
    let mut result = Vec::new();
    for root in order.into_iter().rev() {
        if seen.contains(&root) {
            continue;
        }
        let mut stack = vec![root.clone()];
        let mut component = Vec::new();
        while let Some(n) = stack.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            component.push(n.clone());
            if let Some(next) = rev.get(&n) {
                stack.extend(next.iter().cloned());
            }
        }
        component.sort();
        if component.len() > 1 || adj.get(&root).is_some_and(|n| n.contains(&root)) {
            result.push(component)
        }
    }
    result.sort();
    result
}
