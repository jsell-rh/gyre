//! Subprocess-backed implementation of the [`CallGraphExtractor`] port.
//!
//! Pass 2 of the knowledge-graph pipeline (see `specs/system/lsp-call-graph.md`)
//! delegates to each language's type checker to compute a complete call graph.
//! All of that work requires subprocess I/O, so it lives here in the adapter
//! layer — the domain crate stays free of `Command` (enforced by
//! `scripts/check-arch.sh`).
//!
//! | Language   | Tool                                   |
//! |------------|----------------------------------------|
//! | Go         | `scripts/go-callgraph/go-callgraph` (CHA binary) |
//! | Rust       | `scripts/rust-callgraph.sh` (rust-analyzer LSP)  |
//! | Python     | `scripts/python-callgraph.py` (stdlib `ast`)     |
//! | TypeScript | `scripts/ts-callgraph.mjs` (TypeScript compiler) |
//!
//! Each tool prints a JSON array of `{"from", "to"}` objects to stdout. This
//! adapter parses that into [`CallEdge`]s; resolving the names to graph node
//! IDs is the domain layer's job (`gyre_domain::call_graph_resolve`).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::call_graph::{CallEdge, Language};
use gyre_ports::call_graph::CallGraphExtractor;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

/// Maximum time a Pass 2 tool may run before it is killed and Pass 2 degrades
/// to "no additional edges". Matches the spec's worst-case (~30-60s) budget.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs each language's call-graph tool as a subprocess and parses its output.
#[derive(Debug, Default, Clone)]
pub struct SubprocessCallGraphExtractor;

impl SubprocessCallGraphExtractor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CallGraphExtractor for SubprocessCallGraphExtractor {
    async fn extract_call_edges(
        &self,
        repo_path: &Path,
        language: Language,
    ) -> Result<Vec<CallEdge>> {
        // (program, leading args) — the repo path is appended as the final arg.
        let (program, mut args): (PathBuf, Vec<PathBuf>) = match language {
            Language::Go => match find_go_callgraph_binary() {
                Some(bin) => (bin, Vec::new()),
                None => {
                    tracing::info!("go-callgraph binary not found; skipping Go Pass 2");
                    return Ok(Vec::new());
                }
            },
            Language::Rust => match find_script("rust-callgraph.sh") {
                Some(s) => (PathBuf::from("bash"), vec![s]),
                None => {
                    tracing::info!("rust-callgraph.sh not found; skipping Rust Pass 2");
                    return Ok(Vec::new());
                }
            },
            Language::Python => match find_script("python-callgraph.py") {
                Some(s) => (PathBuf::from("python3"), vec![s]),
                None => {
                    tracing::info!("python-callgraph.py not found; skipping Python Pass 2");
                    return Ok(Vec::new());
                }
            },
            Language::TypeScript => match find_script("ts-callgraph.mjs") {
                Some(s) => (PathBuf::from("node"), vec![s]),
                None => {
                    tracing::info!("ts-callgraph.mjs not found; skipping TypeScript Pass 2");
                    return Ok(Vec::new());
                }
            },
        };
        args.push(repo_path.to_path_buf());

        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                // Missing interpreter (node/python3/bash) or binary — degrade.
                tracing::info!(
                    language = %language,
                    error = %e,
                    "call-graph tool failed to spawn; skipping Pass 2 for this language"
                );
                return Ok(Vec::new());
            }
        };

        let output = match tokio::time::timeout(TOOL_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                tracing::warn!(language = %language, error = %e, "call-graph tool I/O error");
                return Ok(Vec::new());
            }
            Err(_) => {
                tracing::warn!(
                    language = %language,
                    "call-graph tool timed out after {}s; skipping Pass 2",
                    TOOL_TIMEOUT.as_secs()
                );
                return Ok(Vec::new());
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::info!(
                language = %language,
                status = %output.status,
                "call-graph tool exited non-zero: {}",
                stderr.chars().take(500).collect::<String>()
            );
            return Ok(Vec::new());
        }

        let edges: Vec<CallEdge> = match serde_json::from_slice(&output.stdout) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    language = %language,
                    error = %e,
                    "failed to parse call-graph tool JSON output"
                );
                return Ok(Vec::new());
            }
        };

        Ok(edges)
    }
}

/// Locate the `go-callgraph` binary.
///
/// Search order:
/// 1. `GO_CALLGRAPH_BIN` environment variable (explicit override)
/// 2. `scripts/go-callgraph/go-callgraph` under a discovered workspace root
/// 3. `gyre-go-callgraph` / `go-callgraph` on `PATH`
fn find_go_callgraph_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("GO_CALLGRAPH_BIN") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = find_workspace_file("scripts/go-callgraph/go-callgraph") {
        return Some(p);
    }
    for name in ["gyre-go-callgraph", "go-callgraph"] {
        if let Some(p) = which_on_path(name) {
            return Some(p);
        }
    }
    None
}

/// Locate a driver script under `scripts/` in a discovered workspace root.
fn find_script(name: &str) -> Option<PathBuf> {
    find_workspace_file(&format!("scripts/{name}"))
}

/// Find `rel_path` by walking up from candidate roots: the current executable's
/// directory, `CARGO_MANIFEST_DIR` (dev/test builds), and the current working
/// directory. Returns the first existing match.
fn find_workspace_file(rel_path: &str) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        roots.push(PathBuf::from(manifest));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }

    for root in roots {
        let mut dir = root.as_path();
        for _ in 0..8 {
            let candidate = dir.join(rel_path);
            if candidate.exists() {
                return Some(candidate);
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }
    }
    None
}

/// Resolve an executable name on `PATH`.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_repo_returns_empty_for_go() {
        // No go.mod, and (in CI) no binary — must degrade to empty, never error.
        let dir = tempfile::TempDir::new().unwrap();
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(dir.path(), Language::Go)
            .await
            .expect("Pass 2 must never error");
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn missing_interpreter_degrades_gracefully() {
        // Python path: if python3 or the script is missing, we get an empty
        // vec rather than an error.
        let dir = tempfile::TempDir::new().unwrap();
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(dir.path(), Language::Python)
            .await
            .expect("Pass 2 must never error");
        assert!(edges.is_empty());
    }
}
