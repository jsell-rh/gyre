//! Language extractor trait for the knowledge graph extraction pipeline.
//!
//! Implement `LanguageExtractor` for each supported language to produce
//! `GraphNode`s and `GraphEdge`s from a repository checkout.

use gyre_common::graph::{GraphEdge, GraphNode};
use std::path::Path;

/// Result of running a language extractor on a repository.
pub struct ExtractionResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub errors: Vec<ExtractionError>,
}

/// A non-fatal error encountered during extraction.
pub struct ExtractionError {
    pub file_path: String,
    pub message: String,
    pub line: Option<u32>,
}

/// Trait for language-specific extraction of architectural knowledge.
///
/// Each extractor:
/// - detects whether the repository uses its language
/// - walks the source tree and produces graph nodes and edges
///
/// Extractors MUST be deterministic given the same input and MUST NOT
/// perform I/O beyond reading the repository root subtree.
pub trait LanguageExtractor: Send + Sync {
    /// A short human-readable name, e.g. `"rust"`.
    fn name(&self) -> &str;

    /// Return `true` if this extractor should run on `repo_root`.
    ///
    /// Implementors should check for language-specific marker files
    /// (e.g. `Cargo.toml` for Rust, `package.json` for JS).
    fn detect(&self, repo_root: &Path) -> bool;

    /// Walk the repository rooted at `repo_root` and extract graph entities.
    ///
    /// `commit_sha` is the current HEAD SHA (40-char hex), used to populate
    /// `created_sha` / `last_modified_sha` on every emitted node.
    fn extract(&self, repo_root: &Path, commit_sha: &str) -> ExtractionResult;
}
