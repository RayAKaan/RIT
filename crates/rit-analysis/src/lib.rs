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
