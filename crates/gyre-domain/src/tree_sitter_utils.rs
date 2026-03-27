//! Shared tree-sitter parsing utilities for language extractors.
//!
//! Language-specific extractors (Go, Python, TypeScript) use this module to
//! avoid boilerplate tree-sitter setup. Pass the grammar's `Language` object
//! and source bytes to get a parsed `Tree` back.

use tree_sitter::{Language, Parser, Tree};

/// Parse source bytes with the given tree-sitter grammar.
///
/// Returns `Ok(Tree)` on success, or an `Err` string describing the failure.
/// A `None` parse result (which tree-sitter returns when cancelled or timed out)
/// is treated as an error.
pub fn parse_source(source: &[u8], language: Language) -> Result<Tree, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| format!("failed to set tree-sitter language: {e}"))?;
    parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter parse returned None (cancelled or timed out)".to_string())
}
