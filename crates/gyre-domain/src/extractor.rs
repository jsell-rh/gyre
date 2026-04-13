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

/// Scan immediate subdirectories (up to depth 2) for a language marker.
/// Handles monorepo layouts like `src/api/pyproject.toml` or `services/backend/Cargo.toml`.
/// Skips hidden dirs, node_modules, __pycache__, etc.
pub fn shallow_scan_for_marker(root: &Path, check: fn(&Path) -> bool) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "__pycache__"
            || name_str == "target"
            || name_str == "vendor"
        {
            continue;
        }
        if check(&path) {
            return true;
        }
        // One more level (e.g., src/api/)
        if let Ok(sub_entries) = std::fs::read_dir(&path) {
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.is_dir() {
                    let sub_name = sub_entry.file_name();
                    let sub_name_str = sub_name.to_string_lossy();
                    if !sub_name_str.starts_with('.')
                        && sub_name_str != "node_modules"
                        && sub_name_str != "__pycache__"
                        && sub_name_str != "target"
                        && sub_name_str != "vendor"
                        && check(&sub_path)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
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
