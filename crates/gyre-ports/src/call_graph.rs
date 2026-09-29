//! Port trait for Pass 2 call graph extraction.
//!
//! See `specs/system/lsp-call-graph.md`. Pass 1 (syntax extraction) runs in
//! the domain layer via `LanguageExtractor`. Pass 2 delegates to a language
//! type checker (an external binary/script) to compute the complete set of
//! caller→callee edges. Because that requires subprocess I/O, it lives behind
//! this port and is implemented in `gyre-adapters` — the domain layer never
//! spawns subprocesses.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::call_graph::{CallEdge, Language};
use std::path::Path;

/// Extract the complete call graph for a repository via the appropriate
/// language type checker.
///
/// Returns raw [`CallEdge`]s (qualified-name pairs). Resolving those names to
/// graph node IDs is a pure domain concern
/// (`gyre_domain::call_graph_resolve`).
///
/// Implementations MUST degrade gracefully: if the required toolchain or
/// driver script is unavailable, or the repository is not a valid project for
/// `language`, return `Ok(vec![])` rather than an error — Pass 2 is
/// best-effort and never fails a push.
#[async_trait]
pub trait CallGraphExtractor: Send + Sync {
    async fn extract_call_edges(
        &self,
        repo_path: &Path,
        language: Language,
    ) -> Result<Vec<CallEdge>>;
}
