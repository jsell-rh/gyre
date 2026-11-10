//! Shared value types for LSP-powered call graph extraction (Pass 2).
//!
//! See `specs/system/lsp-call-graph.md`. Pass 2 delegates to a language type
//! checker (Go: `go-callgraph` CHA binary; Rust/Python/TypeScript: LSP/compiler
//! driver scripts) that emits a complete list of caller→callee edges as
//! qualified-name pairs. Resolving those names to graph node IDs is a pure
//! domain concern; the raw pairs are carried by [`CallEdge`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Metadata JSON tag written on `Calls` edges produced by Pass 2 (semantic
/// extraction via language type checkers).
///
/// Pass 1 (syntax analysis) leaves `metadata` empty on its `Calls` edges, so
/// the tag distinguishes the two provenances for call-graph completeness
/// metrics (`lsp-call-graph.md` §7–8): `call_edges_pass1` counts live `Calls`
/// edges without the tag, `call_edges_pass2` counts live `Calls` edges with
/// it. Exact-string comparison is safe because the tag is only ever written
/// from this constant.
pub const PASS2_EDGE_METADATA: &str = r#"{"pass":2}"#;

/// Whether a graph edge is a `Calls` edge produced by Pass 2 (semantic
/// extraction), per its [`PASS2_EDGE_METADATA`] tag.
pub fn is_pass2_calls_edge(edge: &crate::graph::GraphEdge) -> bool {
    edge.edge_type == crate::graph::EdgeType::Calls
        && edge.metadata.as_deref() == Some(PASS2_EDGE_METADATA)
}

/// Durable record of the last completed Pass 2 run for a repo
/// (`lsp-call-graph.md` §8–9).
///
/// Written by the extraction pipeline when Pass 2 finishes with positive
/// evidence that a language toolchain actually ran (non-empty raw output);
/// a missing or missing-toolchain run writes nothing, so readers treat the
/// call graph as incomplete. Persisted as JSON in the KV store under
/// namespace `call_graph_status`, key = repo id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphRunRecord {
    /// Unix epoch seconds when Pass 2 completed.
    pub completed_at: u64,
    /// Wall-clock duration of the whole Pass 2 run, in seconds.
    pub duration_secs: f64,
    /// New `Calls` edges persisted by this run.
    pub edges_persisted: usize,
    /// Languages that produced raw output in this run.
    pub languages: Vec<Language>,
}

/// A programming language supported by the call graph extraction pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Go,
    Python,
    TypeScript,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Language::Rust => "rust",
            Language::Go => "go",
            Language::Python => "python",
            Language::TypeScript => "typescript",
        };
        f.write_str(s)
    }
}

/// A raw call edge as reported by a language call-graph tool: a directed edge
/// from one qualified function/method name to another.
///
/// The names use each tool's qualified-name scheme (e.g. Go
/// `<import-path>.<Type>.<Method>`). Resolution of these names to graph node
/// IDs happens in the domain layer (`gyre_domain::call_graph_resolve`).
///
/// Tools may emit additional fields (e.g. a `line` number); they are ignored
/// during deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallEdge {
    pub from: String,
    pub to: String,
}
