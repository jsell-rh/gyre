//! Shared value types for LSP-powered call graph extraction (Pass 2).
//!
//! See `specs/system/lsp-call-graph.md`. Pass 2 delegates to a language type
//! checker (Go: `go-callgraph` CHA binary; Rust/Python/TypeScript: LSP/compiler
//! driver scripts) that emits a complete list of caller→callee edges as
//! qualified-name pairs. Resolving those names to graph node IDs is a pure
//! domain concern; the raw pairs are carried by [`CallEdge`].

use serde::{Deserialize, Serialize};
use std::fmt;

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
