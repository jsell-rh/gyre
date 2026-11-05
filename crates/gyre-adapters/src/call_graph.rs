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
//! | Python     | `scripts/python-callgraph.sh` (pyright LSP)      |
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
            Language::Python => match find_script("python-callgraph.sh") {
                Some(s) => (PathBuf::from("bash"), vec![s]),
                None => {
                    tracing::info!("python-callgraph.sh not found; skipping Python Pass 2");
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

    /// Serializes tests that read or write `GO_CALLGRAPH_BIN`: `set_var`
    /// mutates process-global state, so binary discovery must not race
    /// across tests that exercise the Go path.
    static ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    #[cfg(unix)]
    #[tokio::test]
    async fn env_override_binary_output_is_parsed_into_edges() {
        // Happy path: a GO_CALLGRAPH_BIN stub emitting the documented JSON
        // shape must surface as parsed CallEdges. Exercises discovery step 1
        // plus the stdout -> Vec<CallEdge> path, which the degradation
        // tests never reach.
        let _guard = ENV_LOCK.lock().await;
        let dir = tempfile::TempDir::new().unwrap();
        let stub = dir.path().join("go-callgraph-stub");
        std::fs::write(
            &stub,
            "#!/bin/sh\nprintf '[{\"from\":\"pkg.Caller\",\"to\":\"pkg.Callee\"}]'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        // The stub receives the repo path as its only argument; it ignores
        // it, so an empty temp dir is a sufficient fixture.
        let repo = tempfile::TempDir::new().unwrap();
        std::env::set_var("GO_CALLGRAPH_BIN", &stub);
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(repo.path(), Language::Go)
            .await
            .expect("stub run must succeed");
        std::env::remove_var("GO_CALLGRAPH_BIN");

        assert_eq!(
            edges,
            vec![CallEdge {
                from: "pkg.Caller".to_string(),
                to: "pkg.Callee".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn unknown_repo_returns_empty_for_go() {
        // No go.mod, and (in CI) no binary -- must degrade to empty, never
        // error. Also proves GO_CALLGRAPH_BIN removal restored the default
        // discovery path (temp dirs hold no binary).
        let _guard = ENV_LOCK.lock().await;
        let dir = tempfile::TempDir::new().unwrap();
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(dir.path(), Language::Go)
            .await
            .expect("Pass 2 must never error");
        assert!(edges.is_empty());
    }

    /// Python fixture shared by the pyright tests. Exercises every
    /// Python-specific pattern from the task spec: cross-module call,
    /// method dispatch through a typed receiver, class instantiation,
    /// decorator application, and a module-level call.
    fn write_python_fixture(dir: &Path) {
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = 'fixture'\n").unwrap();
        std::fs::write(dir.join("app/__init__.py"), "").unwrap();
        std::fs::write(
            dir.join("app/util.py"),
            "def helper(x):\n    return x + 1\n",
        )
        .unwrap();
        std::fs::write(dir.join("app/deco.py"), "def wrap(f):\n    return f\n").unwrap();
        std::fs::write(
            dir.join("app/service.py"),
            "class Service:\n    def __init__(self):\n        self.ready = True\n\n    def handle(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("app/main.py"),
            concat!(
                "from app.util import helper\n",
                "from app.deco import wrap\n",
                "from app.service import Service\n",
                "\n",
                "s = Service()\n",
                "\n",
                "@wrap\n",
                "def decorated():\n",
                "    pass\n",
                "\n",
                "def run():\n",
                "    helper(1)\n",
                "    s = Service()\n",
                "    return s.handle()\n",
                "\n",
                "def other():\n",
                "    run()\n",
                "\n",
                "helper(2)\n",
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn missing_pyright_langserver_degrades_to_empty() {
        // Missing pyright binary must degrade to Ok(vec![]) with a warning,
        // never an error -- Pass 2 is best-effort. PYRIGHT_LANGSERVER_BIN
        // pointing at a nonexistent path makes the skip deterministic
        // regardless of whether the host happens to have pyright on PATH.
        let _guard = ENV_LOCK.lock().await;
        let repo = tempfile::TempDir::new().unwrap();
        write_python_fixture(repo.path());
        std::env::set_var("PYRIGHT_LANGSERVER_BIN", "/nonexistent/pyright-langserver");
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(repo.path(), Language::Python)
            .await
            .expect("Pass 2 must never error");
        std::env::remove_var("PYRIGHT_LANGSERVER_BIN");
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn pyright_references_map_to_call_edges() {
        // Full-chain integration test: wrapper -> pyright LSP driver ->
        // textDocument/references -> Calls edges, against a REAL
        // pyright-langserver when one is installed (PYRIGHT_LANGSERVER_BIN
        // or PATH). The deterministic stub-langserver coverage of the same
        // driver contract runs in CI regardless (node --test
        // scripts/dashboard/test/python-callgraph.test.mjs).
        let _guard = ENV_LOCK.lock().await;
        let bin = std::env::var("PYRIGHT_LANGSERVER_BIN")
            .ok()
            .filter(|p| Path::new(p).is_file())
            .map(PathBuf::from)
            .or_else(|| which_on_path("pyright-langserver"));
        let Some(bin) = bin else {
            eprintln!("pyright-langserver not found; skipping real-pyright integration test");
            return;
        };
        let repo = tempfile::TempDir::new().unwrap();
        write_python_fixture(repo.path());
        std::env::set_var("PYRIGHT_LANGSERVER_BIN", &bin);
        let extractor = SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(repo.path(), Language::Python)
            .await
            .expect("pyright Pass 2 must succeed when the server is present");
        std::env::remove_var("PYRIGHT_LANGSERVER_BIN");

        let mut pairs: Vec<(String, String)> = edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("app.main".into(), "app.service.Service".into()),
                ("app.main".into(), "app.util.helper".into()),
                ("app.main.decorated".into(), "app.deco.wrap".into()),
                ("app.main.other".into(), "app.main.run".into()),
                ("app.main.run".into(), "app.service.Service".into()),
                ("app.main.run".into(), "app.service.Service.handle".into()),
                ("app.main.run".into(), "app.util.helper".into()),
            ]
        );
    }
}
