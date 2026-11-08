# Phase 9 — impact foundation

The read-only graph algorithm computes deterministic reverse transitive reachability, handles cycles and reports revision mismatch; it operates only on supplied edges and always marks completeness false. Evidence is attached to each caller-supplied edge, not independently validated. No source extraction, symbol or public API detection, graph persistence, workspace mapping, test recommendation or analysis CLI exists. A result must not be presented as comprehensive impact or evidence that an absent dependency does not exist.

## V2 continuation — bounded syntax extraction

`rit-analysis` now uses pinned Tree-sitter grammars for Python, TypeScript and Go to extract actual static import syntax from explicit UTF-8 source bytes, with input/node/import limits, syntax-error reporting, source locations and `complete=false`. Tests cover each language, comments/strings and malformed syntax. This is a library function, not an authenticated service or CLI, and does not resolve imports into graph edges or analyze a Git revision. Returned snippets can contain source text and must be treated as sensitive. TSX, dynamic imports, symbol/public API and test/workspace impact remain incomplete; no comprehensive claim is made.
