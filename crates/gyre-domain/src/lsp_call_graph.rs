//! LSP-powered call graph extraction (Pass 2).
//!
//! Uses rust-analyzer's LSP protocol to find all references to each
//! function/method definition, resolving complete call edges including:
//! - Cross-module calls through use/import aliases
//! - Trait method calls through dynamic dispatch
//! - Generic instantiations
//! - Re-exported symbols
//!
//! This runs after the syn-based Pass 1 extraction and merges additional
//! Calls edges into the graph.

use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType};
use gyre_common::Id;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read as _, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// RAII guard that kills the child process on drop to prevent leaks.
struct ChildGuard {
    child: Child,
    /// Collected stderr output (drained by a background thread).
    stderr_output: std::sync::Arc<std::sync::Mutex<String>>,
    /// Handle to the stderr draining thread.
    _stderr_thread: Option<std::thread::JoinHandle<()>>,
}

impl ChildGuard {
    fn new(mut child: Child) -> Self {
        // Take stderr and drain it in a background thread to prevent
        // the OS pipe buffer from filling up and deadlocking the child.
        let stderr_output = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stderr_thread = if let Some(stderr) = child.stderr.take() {
            let output = stderr_output.clone();
            Some(std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buf = String::new();
                while reader.read_line(&mut buf).unwrap_or(0) > 0 {
                    if let Ok(mut out) = output.lock() {
                        // Cap at 64KB to avoid unbounded memory growth.
                        if out.len() < 65536 {
                            out.push_str(&buf);
                        }
                    }
                    buf.clear();
                }
            }))
        } else {
            None
        };

        Self {
            child,
            stderr_output,
            _stderr_thread: stderr_thread,
        }
    }

    /// Get collected stderr output (best-effort).
    fn stderr_snapshot(&self) -> String {
        self.stderr_output
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Result of LSP call graph extraction.
pub struct LspCallGraphResult {
    /// Additional Calls edges discovered by LSP.
    pub edges: Vec<GraphEdge>,
    /// Errors encountered during extraction.
    pub errors: Vec<String>,
    /// Number of definitions queried.
    pub definitions_queried: usize,
    /// Number of new call edges found.
    pub new_edges_found: usize,
    /// Total function nodes that should have been queried.
    pub total_definitions: usize,
    /// Whether extraction was cut short by the overall deadline.
    pub incomplete: bool,
    /// Language toolchains that were expected but not found on PATH.
    /// When non-empty, the knowledge graph is missing Calls edges for
    /// these languages — test coverage and blast radius queries will
    /// be incomplete. The frontend should surface this to the user.
    pub missing_toolchains: Vec<String>,
}

/// Check if rust-analyzer is available on the PATH.
pub fn rust_analyzer_available() -> bool {
    Command::new("rust-analyzer")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Extract complete call graph from a Rust repository using rust-analyzer LSP.
///
/// This is Pass 2 of the extraction pipeline. It:
/// 1. Starts rust-analyzer as an LSP subprocess
/// 2. For each function/method node from Pass 1, sends textDocument/didOpen + references
/// 3. For each reference site, resolves the enclosing function → emits Calls edge
/// 4. Deduplicates against existing edges from Pass 1
///
/// Returns additional edges to merge into the graph.
pub fn extract_call_graph(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    let mut result = LspCallGraphResult {
        edges: Vec::new(),
        errors: Vec::new(),
        definitions_queried: 0,
        new_edges_found: 0,
        total_definitions: 0,
        incomplete: false,
        missing_toolchains: vec![],
    };

    if !rust_analyzer_available() {
        result
            .errors
            .push("rust-analyzer not found on PATH — skipping LSP call graph extraction".into());
        result.missing_toolchains.push("rust-analyzer".into());
        return result;
    }

    // Build lookup maps
    let function_nodes: Vec<&GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.deleted_at.is_none()
                && matches!(
                    n.node_type,
                    NodeType::Function | NodeType::Method | NodeType::Endpoint
                )
        })
        .collect();

    if function_nodes.is_empty() {
        return result;
    }

    // Build file → sorted vec of (line_start, line_end, node_id) for resolving reference sites.
    // Only include function-like nodes — modules span entire files and would create
    // invalid "Module calls Function" edges for references outside any function body.
    let mut file_functions: HashMap<String, Vec<(u32, u32, String)>> = HashMap::new();
    for n in nodes.iter().filter(|n| {
        n.deleted_at.is_none()
            && matches!(
                n.node_type,
                NodeType::Function | NodeType::Method | NodeType::Endpoint
            )
    }) {
        file_functions
            .entry(n.file_path.clone())
            .or_default()
            .push((n.line_start, n.line_end, n.id.to_string()));
    }
    // Sort each file's functions by line_start for efficient lookup.
    for fns in file_functions.values_mut() {
        fns.sort_by_key(|(start, _, _)| *start);
    }

    // Collect existing edge pairs to avoid duplicates
    let mut existing_pairs: HashSet<(String, String)> = HashSet::new();
    for e in existing_edges {
        if e.edge_type == EdgeType::Calls && e.deleted_at.is_none() {
            existing_pairs.insert((e.source_id.to_string(), e.target_id.to_string()));
        }
    }

    // Normalize the repo root path for URI construction.
    let repo_root_str = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    // Ensure no trailing slash.
    let repo_root_normalized = repo_root_str.trim_end_matches('/');

    // Start rust-analyzer wrapped in an RAII guard that kills the child on drop.
    let mut guard = match Command::new("rust-analyzer")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(repo_root)
        .spawn()
    {
        Ok(c) => ChildGuard::new(c),
        Err(e) => {
            result
                .errors
                .push(format!("Failed to start rust-analyzer: {e}"));
            return result;
        }
    };

    let stdin = guard.child.stdin.as_mut().unwrap();
    let stdout = guard.child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Per-query timeout: 10 seconds. Overall extraction timeout: 120 seconds.
    let query_timeout = Duration::from_secs(10);
    let overall_deadline = Instant::now() + Duration::from_secs(120);

    // Initialize LSP
    let root_uri = format!("file://{repo_root_normalized}");
    let init_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "references": { "dynamicRegistration": false }
                }
            },
        }
    });

    if let Err(e) = send_lsp_message(stdin, &init_msg) {
        result
            .errors
            .push(format!("Failed to send initialize: {e}"));
        // guard's Drop will kill the child
        return result;
    }

    // Read initialize response
    match read_lsp_message(&mut reader) {
        Ok(Some(_)) => {}
        Ok(None) => {
            result.errors.push("No initialize response".into());
            return result;
        }
        Err(e) => {
            result
                .errors
                .push(format!("Failed to read initialize response: {e}"));
            return result;
        }
    }

    // Send initialized notification
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    let _ = send_lsp_message(stdin, &initialized);

    // Wait for rust-analyzer to finish indexing before querying.
    // rust-analyzer sends `$/progress` notifications during indexing with
    // multiple progress tokens (e.g. "rustAnalyzer/Indexing", "rustAnalyzer/Building CrateGraph").
    // We track all active tokens and wait until ALL have ended, or use a
    // quiescence heuristic: after seeing at least one "end", wait for 2 seconds
    // of no new progress notifications before proceeding.
    let index_deadline = Instant::now() + Duration::from_secs(30);
    let mut active_progress_tokens: HashSet<String> = HashSet::new();
    let mut seen_any_end = false;
    let mut last_progress_time = Instant::now();
    loop {
        if Instant::now() > index_deadline {
            eprintln!("rust-analyzer indexing timeout after 30s, proceeding anyway");
            break;
        }
        // If we've seen at least one "end" and no progress for 2 seconds, consider done.
        if seen_any_end
            && active_progress_tokens.is_empty()
            && last_progress_time.elapsed() >= Duration::from_secs(2)
        {
            eprintln!("rust-analyzer indexing complete (all tokens ended)");
            break;
        }
        let poll_timeout = if seen_any_end {
            // Short poll to detect quiescence quickly
            Duration::from_millis(500)
        } else {
            Duration::from_millis(500)
        };
        match read_lsp_message_with_timeout(&mut reader, poll_timeout) {
            Ok(Some(msg)) => {
                if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                    if method == "$/progress" {
                        last_progress_time = Instant::now();
                        let token = msg
                            .get("params")
                            .and_then(|p| p.get("token"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();
                        if let Some(value) = msg.get("params").and_then(|p| p.get("value")) {
                            match value.get("kind").and_then(|k| k.as_str()) {
                                Some("begin") => {
                                    active_progress_tokens.insert(token);
                                }
                                Some("end") => {
                                    active_progress_tokens.remove(&token);
                                    seen_any_end = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Ok(None) | Err(_) => {
                // Timeout — check quiescence
                if seen_any_end && active_progress_tokens.is_empty() {
                    eprintln!("rust-analyzer indexing complete (quiescence)");
                    break;
                }
            }
        }
    }

    // Track opened files for didOpen notifications.
    let mut opened_files: HashSet<String> = HashSet::new();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    result.total_definitions = function_nodes.len();

    // Query all function nodes. For large repos (~1800 nodes), this completes
    // in ~20 seconds per the spec estimate. The 120s overall deadline caps
    // extraction for very large repos.
    for (idx, func_node) in function_nodes.iter().enumerate() {
        // Check overall deadline to prevent runaway extraction.
        if Instant::now() > overall_deadline {
            result.incomplete = true;
            result.errors.push(format!(
                "Overall extraction timeout after {}/{} definitions ({}% complete, {} edges found)",
                result.definitions_queried,
                result.total_definitions,
                (result.definitions_queried * 100) / result.total_definitions.max(1),
                result.new_edges_found,
            ));
            break;
        }
        result.definitions_queried += 1;

        // Normalize file path: strip leading "./" and ensure no double slashes.
        let normalized_path = func_node
            .file_path
            .strip_prefix("./")
            .unwrap_or(&func_node.file_path);

        // Path traversal guard: canonicalize and verify the resolved path
        // is within repo_root to prevent reading arbitrary files.
        let candidate = repo_root.join(normalized_path);
        let resolved = match safe_resolve_path(repo_root, &candidate) {
            Some(p) => p,
            None => {
                result.errors.push(format!(
                    "Path traversal blocked for {}",
                    func_node.file_path
                ));
                continue;
            }
        };
        let _ = &resolved; // used below via normalized_path which is validated

        let file_uri = format!("file://{repo_root_normalized}/{normalized_path}");

        // Send textDocument/didOpen if we haven't opened this file yet.
        if opened_files.insert(normalized_path.to_string()) {
            // Read the file content for didOpen (path already validated above).
            let file_content = match std::fs::read_to_string(&resolved) {
                Ok(c) => c,
                Err(_) => continue, // Skip files we can't read
            };
            let did_open = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": file_uri,
                        "languageId": "rust",
                        "version": 1,
                        "text": file_content
                    }
                }
            });
            let _ = send_lsp_message(stdin, &did_open);
        }

        // Compute character position: find the function name in the line.
        let char_pos = compute_char_position(repo_root, normalized_path, func_node);

        let refs_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": idx + 1,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": file_uri },
                "position": {
                    "line": func_node.line_start.saturating_sub(1),
                    "character": char_pos
                },
                "context": { "includeDeclaration": false }
            }
        });

        if let Err(e) = send_lsp_message(stdin, &refs_msg) {
            result
                .errors
                .push(format!("Failed to query refs for {}: {e}", func_node.name));
            continue;
        }

        // Read response with timeout
        let deadline = Instant::now() + query_timeout;
        match read_lsp_response_with_timeout(&mut reader, idx + 1, deadline) {
            Ok(Some(locations)) => {
                for loc in locations {
                    if let (Some(uri), Some(line)) = (
                        loc.get("uri").and_then(|u| u.as_str()),
                        loc.get("range")
                            .and_then(|r| r.get("start"))
                            .and_then(|s| s.get("line"))
                            .and_then(|l| l.as_u64()),
                    ) {
                        let prefix = format!("file://{repo_root_normalized}/");
                        let file_path = uri.strip_prefix(&prefix).unwrap_or(uri);
                        let line_num = (line + 1) as u32;

                        // Find the enclosing function at this reference site,
                        // using both line_start and line_end for accurate attribution.
                        if let Some(functions) = file_functions.get(file_path) {
                            let caller_id = functions
                                .iter()
                                .filter(|(start, end, _)| *start <= line_num && *end >= line_num)
                                .max_by_key(|(start, _, _)| *start)
                                .map(|(_, _, id)| id.clone());

                            if let Some(caller) = caller_id {
                                let target = func_node.id.to_string();
                                if caller != target
                                    && !existing_pairs.contains(&(caller.clone(), target.clone()))
                                {
                                    existing_pairs.insert((caller.clone(), target.clone()));
                                    let meta = if commit_sha.is_empty() {
                                        r#"{"source":"lsp"}"#.to_string()
                                    } else {
                                        serde_json::json!({
                                            "source": "lsp",
                                            "commit_sha": commit_sha
                                        })
                                        .to_string()
                                    };
                                    result.edges.push(GraphEdge {
                                        id: Id::new(Uuid::new_v4().to_string()),
                                        repo_id: repo_id.clone(),
                                        source_id: Id::new(caller),
                                        target_id: func_node.id.clone(),
                                        edge_type: EdgeType::Calls,
                                        metadata: Some(meta),
                                        first_seen_at: now,
                                        last_seen_at: now,
                                        deleted_at: None,
                                    });
                                    result.new_edges_found += 1;
                                }
                            }
                        }
                    }
                }
            }
            Ok(None) => {} // No references found
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to read refs for {}: {e}", func_node.name));
            }
        }
    }

    // Shutdown LSP gracefully; ChildGuard's Drop is the safety net.
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 999999,
        "method": "shutdown",
        "params": null
    });
    let _ = send_lsp_message(stdin, &shutdown);
    let _ = read_lsp_message(&mut reader);

    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    let _ = send_lsp_message(stdin, &exit);

    // Wait for exit and capture stderr diagnostics on failure.
    match guard.child.wait() {
        Ok(status) if !status.success() => {
            let stderr = guard.stderr_snapshot();
            let stderr_summary = if stderr.len() > 1024 {
                format!("{}...(truncated)", &stderr[..1024])
            } else {
                stderr
            };
            result.errors.push(format!(
                "rust-analyzer exited with {status}; stderr: {stderr_summary}"
            ));
        }
        Err(e) => {
            result
                .errors
                .push(format!("Failed to wait for rust-analyzer: {e}"));
        }
        _ => {}
    }

    // Scan for RoutesTo edges from Rust route attributes (#[get("/path")], etc.)
    let route_edges =
        extract_rust_route_edges(repo_root, &function_nodes, repo_id, commit_sha, now);
    result.edges.extend(route_edges);

    result
}

/// Scan Rust source files for HTTP route attributes (actix-web / axum style)
/// and emit RoutesTo edges from the handler function node to the route path.
///
/// Recognizes patterns like:
///   #[get("/api/users")]
///   #[post("/api/items")]
///   #[put("/api/items/{id}")]
///   #[delete("/api/items/{id}")]
///   #[patch("/api/items/{id}")]
///   #[route("/api/health", method = "GET")]
fn extract_rust_route_edges(
    repo_root: &Path,
    function_nodes: &[&GraphNode],
    repo_id: &Id,
    commit_sha: &str,
    now: u64,
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let route_methods = [
        "get", "post", "put", "delete", "patch", "head", "options", "route",
    ];

    // Group function nodes by file path for efficient scanning
    let mut nodes_by_file: HashMap<&str, Vec<&GraphNode>> = HashMap::new();
    for node in function_nodes {
        nodes_by_file
            .entry(node.file_path.as_str())
            .or_default()
            .push(node);
    }

    for (file_path, file_nodes) in &nodes_by_file {
        let normalized = file_path.strip_prefix("./").unwrap_or(file_path);
        let full_path = repo_root.join(normalized);
        let resolved = match safe_resolve_path(repo_root, &full_path) {
            Some(p) => p,
            None => continue,
        };
        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();

        // For each function node, check lines above its definition for route attributes
        for node in file_nodes {
            let fn_line_idx = node.line_start.saturating_sub(1) as usize;
            // Scan up to 10 lines above the function definition for attributes
            let start = fn_line_idx.saturating_sub(10);
            for i in (start..fn_line_idx).rev() {
                let line = match lines.get(i) {
                    Some(l) => l.trim(),
                    None => break,
                };
                // Stop scanning if we hit a non-attribute, non-empty, non-comment line
                if !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("//")
                    && !line.starts_with("///")
                {
                    break;
                }
                // Check for route attribute pattern: #[method("path")]
                if !line.starts_with("#[") {
                    continue;
                }
                for method in &route_methods {
                    // Match #[get("...")] or #[actix_web::get("...")]
                    let patterns = [
                        format!("#[{}(\"", method),
                        format!("#[actix_web::{}(\"", method),
                        format!("#[axum::routing::{}(\"", method),
                    ];
                    for pattern in &patterns {
                        if let Some(start_pos) = line.find(pattern.as_str()) {
                            let path_start = start_pos + pattern.len();
                            if let Some(path_end) = line[path_start..].find('"') {
                                let route_path = &line[path_start..path_start + path_end];
                                let meta = if commit_sha.is_empty() {
                                    serde_json::json!({
                                        "source": "syntax",
                                        "http_method": method.to_uppercase(),
                                        "route_path": route_path
                                    })
                                    .to_string()
                                } else {
                                    serde_json::json!({
                                        "source": "syntax",
                                        "commit_sha": commit_sha,
                                        "http_method": method.to_uppercase(),
                                        "route_path": route_path
                                    })
                                    .to_string()
                                };
                                edges.push(GraphEdge {
                                    id: Id::new(Uuid::new_v4().to_string()),
                                    repo_id: repo_id.clone(),
                                    source_id: node.id.clone(),
                                    target_id: Id::new(format!(
                                        "route:{}:{}",
                                        method.to_uppercase(),
                                        route_path
                                    )),
                                    edge_type: EdgeType::RoutesTo,
                                    metadata: Some(meta),
                                    first_seen_at: now,
                                    last_seen_at: now,
                                    deleted_at: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    edges
}

/// Compute the character position of a function name in its definition line.
///
/// Handles all Rust function modifiers (`pub`, `pub(crate)`, `async`, `const`,
/// `unsafe`, `extern "C"`, and combinations thereof) by searching for the
/// substring `fn <name>` anywhere in the line.  Falls back to the last
/// occurrence of the name if `fn <name>` is not found (e.g. for methods
/// defined via macros), and finally to column 0.
fn compute_char_position(repo_root: &Path, file_path: &str, node: &GraphNode) -> u32 {
    let full_path = repo_root.join(file_path);
    // Validate the resolved path is within repo_root.
    let resolved = match safe_resolve_path(repo_root, &full_path) {
        Some(p) => p,
        None => return 0,
    };
    let Ok(content) = std::fs::read_to_string(&resolved) else {
        return 0;
    };
    let line_idx = node.line_start.saturating_sub(1) as usize;
    let Some(line) = content.lines().nth(line_idx) else {
        return 0;
    };

    // Try language-specific keyword patterns to find the function name.
    // Each pattern matches "keyword <name>" and returns position at name start.
    // Covers Rust (fn), Python (def/async def), Go (func), JS/TS (function/async function).
    let keywords = [
        "fn ",
        "def ",
        "func ",
        "function ",
        "async def ",
        "async function ",
    ];
    for kw in &keywords {
        let needle = format!("{}{}", kw, node.name);
        if let Some(pos) = line.find(&needle) {
            let after = pos + needle.len();
            let next_char = line.as_bytes().get(after).copied();
            let is_exact = match next_char {
                None => true,
                Some(c) => !c.is_ascii_alphanumeric() && c != b'_',
            };
            if is_exact {
                return (pos + kw.len()) as u32;
            }
        }
    }

    // Fallback: find the FIRST word-boundary-delimited occurrence of the name
    // in the line. We require that the character before and after the match is
    // not alphanumeric or underscore, preventing "new" from matching inside
    // "new_connection" or "renewal".
    let name_bytes = node.name.as_bytes();
    let line_bytes = line.as_bytes();
    let mut search_start = 0;
    while let Some(offset) = line[search_start..].find(&node.name) {
        let pos = search_start + offset;
        let before_ok = if pos == 0 {
            true
        } else {
            let c = line_bytes[pos - 1];
            !c.is_ascii_alphanumeric() && c != b'_'
        };
        let after_pos = pos + name_bytes.len();
        let after_ok = if after_pos >= line_bytes.len() {
            true
        } else {
            let c = line_bytes[after_pos];
            !c.is_ascii_alphanumeric() && c != b'_'
        };
        if before_ok && after_ok {
            return pos as u32;
        }
        search_start = pos + 1;
    }
    0
}

/// Resolve `candidate` to an absolute path and verify it is within `repo_root`.
///
/// Returns `None` if the path escapes the repo root (e.g. via `../`).
fn safe_resolve_path(repo_root: &Path, candidate: &Path) -> Option<PathBuf> {
    let canon_root = repo_root.canonicalize().ok()?;
    let canon_path = candidate.canonicalize().ok()?;
    if canon_path.starts_with(&canon_root) {
        Some(canon_path)
    } else {
        None
    }
}

fn send_lsp_message(
    stdin: &mut std::process::ChildStdin,
    msg: &serde_json::Value,
) -> std::io::Result<()> {
    let body = serde_json::to_string(msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes())?;
    stdin.write_all(body.as_bytes())?;
    stdin.flush()
}

fn read_lsp_message(
    reader: &mut BufReader<std::process::ChildStdout>,
) -> std::io::Result<Option<serde_json::Value>> {
    // Read headers
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len) = trimmed.strip_prefix("Content-Length: ") {
            content_length = len.parse::<usize>().unwrap_or(0);
        }
    }

    if content_length == 0 {
        return Ok(None);
    }

    // Cap Content-Length to prevent OOM from malicious/buggy LSP responses (max 64MB)
    const MAX_LSP_CONTENT_LENGTH: usize = 64 * 1024 * 1024;
    if content_length > MAX_LSP_CONTENT_LENGTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "LSP Content-Length {} exceeds maximum {} bytes",
                content_length, MAX_LSP_CONTENT_LENGTH
            ),
        ));
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(parsed))
}

/// Read a single LSP message with a timeout.
///
/// Uses `poll(2)` on the stdout file descriptor to wait for data availability
/// before attempting the blocking read. Returns `Ok(None)` on timeout rather
/// than an error so callers can retry without treating it as fatal.
fn read_lsp_message_with_timeout(
    reader: &mut BufReader<std::process::ChildStdout>,
    timeout: Duration,
) -> std::io::Result<Option<serde_json::Value>> {
    use std::os::unix::io::AsRawFd;

    // If the BufReader already has a complete LSP header buffered, skip the poll
    // and read directly. We check for "\r\n\r\n" (header terminator) — if only a
    // partial header is buffered, we still need to poll for more data to avoid
    // blocking indefinitely in read_lsp_message's read_line loop.
    {
        let buf = reader.buffer();
        if !buf.is_empty() {
            // Check if we have a complete header (contains \r\n\r\n)
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                return read_lsp_message(reader);
            }
            // Partial header buffered — fall through to poll for more data
        }
    }

    // Use poll(2) to wait for data on the stdout fd with a timeout.
    let fd = reader.get_ref().as_raw_fd();
    // Clamp to i32::MAX (~24.8 days) to prevent overflow wrapping to negative
    // which would cause poll to return immediately.
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;

    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    // SAFETY: pollfd is a valid stack-allocated struct, nfds=1 is correct.
    let ret = unsafe { libc::poll(&mut pollfd as *mut libc::pollfd, 1, timeout_ms) };

    if ret <= 0 {
        // 0 = timeout, negative = error (treat as timeout for simplicity)
        return Ok(None);
    }

    if pollfd.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
        read_lsp_message(reader)
    } else {
        Ok(None)
    }
}

fn read_lsp_response_with_timeout(
    reader: &mut BufReader<std::process::ChildStdout>,
    expected_id: usize,
    deadline: Instant,
) -> std::io::Result<Option<Vec<serde_json::Value>>> {
    // Read messages until we get the response with our ID or timeout.
    // Uses poll-based read_lsp_message_with_timeout per message to avoid
    // blocking indefinitely on a stuck LSP server.
    for _ in 0..100 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "LSP response timeout",
            ));
        }

        // Use at most 2 seconds per individual message read, capped by the overall deadline.
        let per_msg_timeout = remaining.min(Duration::from_secs(2));

        match read_lsp_message_with_timeout(reader, per_msg_timeout)? {
            Some(msg) => {
                if let Some(id) = msg.get("id").and_then(|i| i.as_u64()) {
                    if id as usize == expected_id {
                        if let Some(result) = msg.get("result") {
                            if let Some(arr) = result.as_array() {
                                return Ok(Some(arr.clone()));
                            }
                            return Ok(None);
                        }
                        return Ok(None);
                    }
                }
                // Not our response — continue reading (likely a notification)
            }
            None => {
                // Timeout on this message — check overall deadline
                if Instant::now() > deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "LSP response timeout",
                    ));
                }
            }
        }
    }
    Ok(None) // Gave up after too many messages
}

// ── Multi-language support ──────────────────────────────────────────────────
// The extractors below follow the same LSP pattern as the Rust extractor
// but delegate to pyright (Python), gopls (Go), and typescript-language-server (TypeScript).

/// Detected primary language of a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoLanguage {
    Rust,
    Python,
    Go,
    TypeScript,
    Unknown,
}

/// Detect the primary language of a repository by checking for manifest files.
///
/// Checks the repo root first, then scans immediate subdirectories (depth=1)
/// for additional language manifests. This handles monorepos where language
/// roots are one level down (e.g., `backend/Cargo.toml`, `frontend/package.json`).
pub fn detect_language(repo_root: &Path) -> RepoLanguage {
    if has_rust_manifest(repo_root) {
        RepoLanguage::Rust
    } else if has_go_manifest(repo_root) {
        RepoLanguage::Go
    } else if has_python_manifest(repo_root) {
        RepoLanguage::Python
    } else if has_typescript_manifest(repo_root) {
        RepoLanguage::TypeScript
    } else {
        // Scan immediate subdirectories (depth=1) for manifest files.
        if let Ok(entries) = std::fs::read_dir(repo_root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if has_rust_manifest(&path) {
                    return RepoLanguage::Rust;
                }
                if has_go_manifest(&path) {
                    return RepoLanguage::Go;
                }
                if has_python_manifest(&path) {
                    return RepoLanguage::Python;
                }
                if has_typescript_manifest(&path) {
                    return RepoLanguage::TypeScript;
                }
            }
        }
        RepoLanguage::Unknown
    }
}

fn has_rust_manifest(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
}

fn has_go_manifest(dir: &Path) -> bool {
    dir.join("go.mod").is_file()
}

fn has_python_manifest(dir: &Path) -> bool {
    dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("requirements.txt").is_file()
}

fn has_typescript_manifest(dir: &Path) -> bool {
    dir.join("tsconfig.json").is_file() || dir.join("package.json").is_file()
}

/// Detect ALL languages present in a polyglot repository.
///
/// Checks the repo root and immediate subdirectories (depth=1) for manifest
/// files, returning all detected languages. This handles monorepos where
/// different languages live in subdirectories.
pub fn detect_all_languages(repo_root: &Path) -> Vec<RepoLanguage> {
    let mut has_rust = false;
    let mut has_go = false;
    let mut has_python = false;
    let mut has_ts = false;

    // Check directories: repo root + immediate children
    let mut dirs_to_check: Vec<PathBuf> = vec![repo_root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(repo_root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                dirs_to_check.push(path);
            }
        }
    }

    for dir in &dirs_to_check {
        if !has_rust && has_rust_manifest(dir) {
            has_rust = true;
        }
        if !has_go && has_go_manifest(dir) {
            has_go = true;
        }
        if !has_python && has_python_manifest(dir) {
            has_python = true;
        }
        if !has_ts && has_typescript_manifest(dir) {
            has_ts = true;
        }
    }

    let mut languages = Vec::new();
    if has_rust {
        languages.push(RepoLanguage::Rust);
    }
    if has_go {
        languages.push(RepoLanguage::Go);
    }
    if has_python {
        languages.push(RepoLanguage::Python);
    }
    if has_ts {
        languages.push(RepoLanguage::TypeScript);
    }
    languages
}

/// Check if pyright is available.
pub fn pyright_available() -> bool {
    Command::new("pyright-langserver")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or_else(|_| {
            // Try npx pyright as fallback
            Command::new("npx")
                .args(["pyright-langserver", "--version"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
}

/// Check if gopls is available.
pub fn gopls_available() -> bool {
    Command::new("gopls")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if typescript-language-server is available.
pub fn tsserver_available() -> bool {
    Command::new("typescript-language-server")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Generic LSP-based call graph extraction that works for any language server.
///
/// The pattern is identical across languages:
/// 1. Start the language server subprocess
/// 2. Initialize with LSP protocol
/// 3. For each function node, send textDocument/references
/// 4. Resolve reference sites to enclosing functions → emit Calls edges
fn extract_call_graph_via_lsp(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
    lsp_command: &str,
    lsp_args: &[&str],
    language_id: &str,
    file_extensions: &[&str],
) -> LspCallGraphResult {
    let mut result = LspCallGraphResult {
        edges: Vec::new(),
        errors: Vec::new(),
        definitions_queried: 0,
        new_edges_found: 0,
        total_definitions: 0,
        incomplete: false,
        missing_toolchains: vec![],
    };

    let matches_ext = |path: &str| file_extensions.iter().any(|ext| path.ends_with(ext));

    // Build lookup maps
    let function_nodes: Vec<&GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.deleted_at.is_none()
                && matches!(
                    n.node_type,
                    NodeType::Function | NodeType::Method | NodeType::Endpoint
                )
                && matches_ext(&n.file_path)
        })
        .collect();

    if function_nodes.is_empty() {
        return result;
    }

    // Build file → sorted vec of (line_start, line_end, node_id)
    // Only include function-like nodes — modules span entire files and would create
    // invalid "Module calls Function" edges for references outside any function body.
    let mut file_functions: HashMap<String, Vec<(u32, u32, String)>> = HashMap::new();
    for n in nodes.iter().filter(|n| {
        n.deleted_at.is_none()
            && matches!(
                n.node_type,
                NodeType::Function | NodeType::Method | NodeType::Endpoint
            )
    }) {
        if !matches_ext(&n.file_path) {
            continue;
        }
        file_functions
            .entry(n.file_path.clone())
            .or_default()
            .push((n.line_start, n.line_end, n.id.to_string()));
    }
    for fns in file_functions.values_mut() {
        fns.sort_by_key(|(start, _, _)| *start);
    }

    // Collect existing edge pairs
    let mut existing_pairs: HashSet<(String, String)> = HashSet::new();
    for e in existing_edges {
        if e.edge_type == EdgeType::Calls && e.deleted_at.is_none() {
            existing_pairs.insert((e.source_id.to_string(), e.target_id.to_string()));
        }
    }

    let repo_root_str = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    let repo_root_normalized = repo_root_str.trim_end_matches('/');

    // Start LSP server
    let mut cmd = Command::new(lsp_command);
    cmd.args(lsp_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(repo_root);
    let mut guard = match cmd.spawn() {
        Ok(c) => ChildGuard::new(c),
        Err(e) => {
            result
                .errors
                .push(format!("Failed to start {lsp_command}: {e}"));
            return result;
        }
    };

    let stdin = guard.child.stdin.as_mut().unwrap();
    let stdout = guard.child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let query_timeout = Duration::from_secs(10);
    let overall_deadline = Instant::now() + Duration::from_secs(120);

    // Initialize LSP
    let root_uri = format!("file://{repo_root_normalized}");
    let init_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "references": { "dynamicRegistration": false }
                }
            },
        }
    });

    if let Err(e) = send_lsp_message(stdin, &init_msg) {
        result
            .errors
            .push(format!("Failed to send initialize: {e}"));
        return result;
    }

    match read_lsp_message(&mut reader) {
        Ok(Some(_)) => {}
        Ok(None) => {
            result.errors.push("No initialize response".into());
            return result;
        }
        Err(e) => {
            result
                .errors
                .push(format!("Failed to read initialize response: {e}"));
            return result;
        }
    }

    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    let _ = send_lsp_message(stdin, &initialized);

    // Wait for indexing (with shorter timeout than Rust)
    let index_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > index_deadline {
            break;
        }
        match read_lsp_message_with_timeout(&mut reader, Duration::from_millis(500)) {
            Ok(Some(msg)) => {
                if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                    if method == "$/progress" {
                        if let Some(value) = msg.get("params").and_then(|p| p.get("value")) {
                            if value.get("kind").and_then(|k| k.as_str()) == Some("end") {
                                break;
                            }
                        }
                    }
                }
            }
            Ok(None) | Err(_) => continue,
        }
    }

    let mut opened_files: HashSet<String> = HashSet::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    result.total_definitions = function_nodes.len();

    for (idx, func_node) in function_nodes.iter().enumerate() {
        if Instant::now() > overall_deadline {
            result.incomplete = true;
            result.errors.push(format!(
                "Overall extraction timeout after {}/{} definitions",
                result.definitions_queried, result.total_definitions,
            ));
            break;
        }
        result.definitions_queried += 1;

        let normalized_path = func_node
            .file_path
            .strip_prefix("./")
            .unwrap_or(&func_node.file_path);
        let candidate = repo_root.join(normalized_path);
        let resolved = match safe_resolve_path(repo_root, &candidate) {
            Some(p) => p,
            None => continue,
        };

        let file_uri = format!("file://{repo_root_normalized}/{normalized_path}");

        if opened_files.insert(normalized_path.to_string()) {
            let file_content = match std::fs::read_to_string(&resolved) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Infer languageId from file extension for correct LSP handling
            // (e.g., TypeScript LSP needs "typescriptreact" for .tsx, not "typescript")
            let inferred_lang_id = match func_node.file_path.rsplit('.').next() {
                Some("ts") => "typescript",
                Some("tsx") => "typescriptreact",
                Some("js") => "javascript",
                Some("jsx") => "javascriptreact",
                _ => language_id,
            };
            let did_open = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": file_uri,
                        "languageId": inferred_lang_id,
                        "version": 1,
                        "text": file_content
                    }
                }
            });
            let _ = send_lsp_message(stdin, &did_open);
        }

        // Compute character position for the function name in the definition line.
        // This correctly handles language-specific keywords (def, func, export function, etc.)
        // by finding the function name substring in the source line.
        let char_pos = compute_char_position(repo_root, &func_node.file_path, func_node);
        let refs_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": idx + 1,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": file_uri },
                "position": {
                    "line": func_node.line_start.saturating_sub(1),
                    "character": char_pos
                },
                "context": { "includeDeclaration": false }
            }
        });

        if let Err(e) = send_lsp_message(stdin, &refs_msg) {
            result
                .errors
                .push(format!("Failed to query refs for {}: {e}", func_node.name));
            continue;
        }

        let deadline = Instant::now() + query_timeout;
        match read_lsp_response_with_timeout(&mut reader, idx + 1, deadline) {
            Ok(Some(locations)) => {
                for loc in locations {
                    if let (Some(uri), Some(line)) = (
                        loc.get("uri").and_then(|u| u.as_str()),
                        loc.get("range")
                            .and_then(|r| r.get("start"))
                            .and_then(|s| s.get("line"))
                            .and_then(|l| l.as_u64()),
                    ) {
                        let prefix = format!("file://{repo_root_normalized}/");
                        let file_path = uri.strip_prefix(&prefix).unwrap_or(uri);
                        let line_num = (line + 1) as u32;

                        if let Some(functions) = file_functions.get(file_path) {
                            let caller_id = functions
                                .iter()
                                .filter(|(start, end, _)| *start <= line_num && *end >= line_num)
                                .max_by_key(|(start, _, _)| *start)
                                .map(|(_, _, id)| id.clone());

                            if let Some(caller) = caller_id {
                                let target = func_node.id.to_string();
                                if caller != target
                                    && !existing_pairs.contains(&(caller.clone(), target.clone()))
                                {
                                    existing_pairs.insert((caller.clone(), target.clone()));
                                    let meta = if commit_sha.is_empty() {
                                        r#"{"source":"lsp"}"#.to_string()
                                    } else {
                                        serde_json::json!({
                                            "source": "lsp",
                                            "commit_sha": commit_sha
                                        })
                                        .to_string()
                                    };
                                    result.edges.push(GraphEdge {
                                        id: Id::new(Uuid::new_v4().to_string()),
                                        repo_id: repo_id.clone(),
                                        source_id: Id::new(caller),
                                        target_id: func_node.id.clone(),
                                        edge_type: EdgeType::Calls,
                                        metadata: Some(meta),
                                        first_seen_at: now,
                                        last_seen_at: now,
                                        deleted_at: None,
                                    });
                                    result.new_edges_found += 1;
                                }
                            }
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to read refs for {}: {e}", func_node.name));
            }
        }
    }

    // Shutdown
    let shutdown =
        serde_json::json!({ "jsonrpc": "2.0", "id": 999999, "method": "shutdown", "params": null });
    let _ = send_lsp_message(stdin, &shutdown);
    let _ = read_lsp_message(&mut reader);
    let exit = serde_json::json!({ "jsonrpc": "2.0", "method": "exit", "params": null });
    let _ = send_lsp_message(stdin, &exit);

    match guard.child.wait() {
        Ok(status) if !status.success() => {
            let stderr = guard.stderr_snapshot();
            let summary = if stderr.len() > 1024 {
                format!("{}...", &stderr[..1024])
            } else {
                stderr
            };
            result.errors.push(format!(
                "{lsp_command} exited with {status}; stderr: {summary}"
            ));
        }
        Err(e) => {
            result
                .errors
                .push(format!("Failed to wait for {lsp_command}: {e}"));
        }
        _ => {}
    }

    result
}

/// Extract call graph from a Python repository using pyright LSP.
pub fn extract_call_graph_python(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    if !pyright_available() {
        return LspCallGraphResult {
            edges: Vec::new(),
            errors: vec!["pyright-langserver not found — skipping Python LSP call graph".into()],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: false,
            missing_toolchains: vec!["pyright".into()],
        };
    }

    extract_call_graph_via_lsp(
        repo_root,
        nodes,
        existing_edges,
        repo_id,
        commit_sha,
        "pyright-langserver",
        &["--stdio"],
        "python",
        &[".py"],
    )
}

/// Extract call graph from a Go repository using gopls LSP.
pub fn extract_call_graph_go(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    // Prefer the dedicated go-callgraph binary (uses golang.org/x/tools/go/callgraph CHA)
    // which produces a complete call graph in a single pass — the gold standard per spec.
    // Falls back to gopls LSP if the binary is not available.
    if let Some(result) =
        try_go_callgraph_binary(repo_root, nodes, existing_edges, repo_id, commit_sha)
    {
        return result;
    }

    if !gopls_available() {
        return LspCallGraphResult {
            edges: Vec::new(),
            errors: vec![
                "Neither gyre-go-callgraph nor gopls found — skipping Go call graph".into(),
            ],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: false,
            missing_toolchains: vec!["gyre-go-callgraph".into(), "gopls".into()],
        };
    }

    extract_call_graph_via_lsp(
        repo_root,
        nodes,
        existing_edges,
        repo_id,
        commit_sha,
        "gopls",
        &["serve"],
        "go",
        &[".go"],
    )
}

/// Try the dedicated go-callgraph binary (CHA-based, complete call graph).
/// Returns None if the binary is not found, Some(result) otherwise.
fn try_go_callgraph_binary(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    _commit_sha: &str,
) -> Option<LspCallGraphResult> {
    // Look for the binary in multiple locations:
    // 1. Relative to executable (production deployment)
    // 2. CARGO_MANIFEST_DIR ancestor's scripts/ (development builds via cargo)
    // 3. Current working directory's scripts/ (running from repo root)
    // 4. In PATH as 'gyre-go-callgraph'
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let cwd_scripts = std::env::current_dir()
        .ok()
        .map(|d| d.join("scripts/go-callgraph/go-callgraph"));
    // Walk up from exe dir to find workspace root (contains scripts/)
    let workspace_scripts = exe_dir.as_ref().and_then(|d| {
        let mut dir = d.as_path();
        for _ in 0..5 {
            let candidate = dir.join("scripts/go-callgraph/go-callgraph");
            if candidate.exists() {
                return Some(candidate);
            }
            dir = dir.parent()?;
        }
        None
    });
    let binary_candidates = [
        exe_dir
            .as_ref()
            .map(|d| d.join("scripts/go-callgraph/go-callgraph")),
        workspace_scripts,
        cwd_scripts,
        Some(PathBuf::from("gyre-go-callgraph")),
    ];

    let binary_path = binary_candidates.iter().flatten().find(|p| {
        // Check if executable exists (for absolute paths) or is in PATH
        if p.is_absolute() {
            p.exists()
        } else {
            Command::new(p)
                .arg("--help")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        }
    });

    let binary_path = match binary_path {
        Some(p) => p.clone(),
        None => return None,
    };

    // Run the binary with the repo path and wait with a proper timeout.
    // We spawn a thread to call wait_with_output (which blocks) and use
    // condvar-based wait_timeout on the main thread to enforce the deadline
    // without busy-polling.
    let child = match Command::new(&binary_path)
        .arg(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Some(LspCallGraphResult {
                edges: Vec::new(),
                errors: vec![format!("go-callgraph binary failed to spawn: {e}")],
                definitions_queried: 0,
                new_edges_found: 0,
                total_definitions: 0,
                incomplete: true,
                missing_toolchains: Vec::new(),
            });
        }
    };

    // Use a condvar to wait for the child to finish with a 60s timeout,
    // avoiding the previous busy-wait polling loop.
    let pair = std::sync::Arc::new((
        std::sync::Mutex::new(None::<std::io::Result<std::process::Output>>),
        std::sync::Condvar::new(),
    ));
    let pair_clone = pair.clone();
    let wait_thread = std::thread::spawn(move || {
        let result = child.wait_with_output();
        let (lock, cvar) = &*pair_clone;
        if let Ok(mut output) = lock.lock() {
            *output = Some(result);
        }
        cvar.notify_one();
    });

    let (lock, cvar) = &*pair;
    let timeout = Duration::from_secs(60);
    let guard = lock.lock().unwrap();
    let (mut guard, wait_result) = cvar
        .wait_timeout_while(guard, timeout, |o| o.is_none())
        .unwrap();

    let output = if wait_result.timed_out() && guard.is_none() {
        // The child is still running in the wait_thread; we cannot kill it
        // directly since wait_with_output consumed it. Detach the thread
        // and report timeout.
        drop(guard);
        let _ = wait_thread.join();
        return Some(LspCallGraphResult {
            edges: Vec::new(),
            errors: vec!["go-callgraph timed out after 60s".into()],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: true,
            missing_toolchains: Vec::new(),
        });
    } else {
        match guard.take().unwrap() {
            Ok(output) => output,
            Err(e) => {
                return Some(LspCallGraphResult {
                    edges: Vec::new(),
                    errors: vec![format!("go-callgraph wait error: {e}")],
                    definitions_queried: 0,
                    new_edges_found: 0,
                    total_definitions: 0,
                    incomplete: true,
                    missing_toolchains: Vec::new(),
                });
            }
        }
    };
    let _ = wait_thread.join();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Non-zero exit could mean no Go module found — fall back to gopls
        if stderr.contains("no packages") || stderr.contains("go.mod") {
            return None;
        }
        return Some(LspCallGraphResult {
            edges: Vec::new(),
            errors: vec![format!(
                "go-callgraph exited with {}: {}",
                output.status,
                stderr.chars().take(500).collect::<String>()
            )],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: true,
            missing_toolchains: Vec::new(),
        });
    }

    // Parse JSON output: [{"from": "pkg.Func", "to": "pkg.Other"}, ...]
    #[derive(serde::Deserialize)]
    struct GoCallEdge {
        from: String,
        to: String,
    }

    let call_edges: Vec<GoCallEdge> = match serde_json::from_slice(&output.stdout) {
        Ok(edges) => edges,
        Err(e) => {
            return Some(LspCallGraphResult {
                edges: Vec::new(),
                errors: vec![format!("Failed to parse go-callgraph output: {e}")],
                definitions_queried: 0,
                new_edges_found: 0,
                total_definitions: 0,
                incomplete: true,
                missing_toolchains: Vec::new(),
            });
        }
    };

    // Build node lookup by qualified name for matching
    let node_by_qname: HashMap<&str, &GraphNode> = nodes
        .iter()
        .filter(|n| n.deleted_at.is_none() && !n.qualified_name.is_empty())
        .map(|n| (n.qualified_name.as_str(), n))
        .collect();

    // Also build by qualified_name for fallback matching — keying by
    // qualified_name avoids collisions between methods with the same short
    // name on different types (e.g., FooService.Handle vs BarService.Handle).
    let mut node_by_name: HashMap<&str, Vec<&GraphNode>> = HashMap::new();
    for n in nodes.iter().filter(|n| {
        n.deleted_at.is_none() && matches!(n.node_type, NodeType::Function | NodeType::Endpoint)
    }) {
        node_by_name
            .entry(n.qualified_name.as_str())
            .or_default()
            .push(n);
    }

    // Build existing edge set for dedup
    let existing_set: HashSet<(String, String)> = existing_edges
        .iter()
        .filter(|e| e.deleted_at.is_none() && e.edge_type == EdgeType::Calls)
        .map(|e| (e.source_id.to_string(), e.target_id.to_string()))
        .collect();

    let mut new_edges = Vec::new();
    let total_go_edges = call_edges.len();

    for ge in &call_edges {
        // Try to resolve from/to to graph nodes
        let from_node = resolve_go_node(&ge.from, &node_by_qname, &node_by_name);
        let to_node = resolve_go_node(&ge.to, &node_by_qname, &node_by_name);

        if let (Some(from), Some(to)) = (from_node, to_node) {
            let key = (from.id.to_string(), to.id.to_string());
            if !existing_set.contains(&key) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                new_edges.push(GraphEdge {
                    id: Id::new(&Uuid::new_v4().to_string()),
                    repo_id: repo_id.clone(),
                    source_id: from.id.clone(),
                    target_id: to.id.clone(),
                    edge_type: EdgeType::Calls,
                    metadata: None,
                    first_seen_at: now,
                    last_seen_at: now,
                    deleted_at: None,
                });
            }
        }
    }

    let new_count = new_edges.len();
    Some(LspCallGraphResult {
        edges: new_edges,
        errors: Vec::new(),
        definitions_queried: total_go_edges,
        new_edges_found: new_count,
        total_definitions: total_go_edges,
        incomplete: false,
        missing_toolchains: Vec::new(),
    })
}

/// Resolve a Go qualified name (e.g., "pkg/path.TypeName.MethodName") to a graph node.
/// `by_name` is keyed by qualified_name so we suffix-match instead of exact-lookup.
///
/// Resolution strategy:
/// 1. Exact qualified name match (most reliable)
/// 2. For methods (Receiver.Method), require the full "Receiver.Method" suffix to match
///    (not just the method name alone, which causes false positives like
///    FooService.Handle matching BarService.Handle)
/// 3. When multiple candidates match, prefer one in the same package directory
///    as inferred from the caller's package path in the qualified name
fn resolve_go_node<'a>(
    qualified: &str,
    by_qname: &HashMap<&str, &'a GraphNode>,
    by_name: &HashMap<&str, Vec<&'a GraphNode>>,
) -> Option<&'a GraphNode> {
    // Direct qualified name match (most reliable)
    if let Some(n) = by_qname.get(qualified) {
        return Some(n);
    }
    // Also try direct lookup in by_name (exact qualified_name match)
    if let Some(candidates) = by_name.get(qualified) {
        return Some(candidates[0]);
    }

    // Extract the package path prefix (everything before the last '.') for
    // same-package preference when disambiguating multiple candidates.
    let pkg_prefix = qualified.rsplit_once('.').map(|(prefix, _)| prefix);

    // Try TypeName.MethodName pattern first (more specific than bare name).
    // For "pkg/path.TypeName.MethodName", extract "TypeName.MethodName" as
    // the receiver-qualified method path.
    let parts: Vec<&str> = qualified.rsplitn(3, '.').collect();
    if parts.len() >= 2 {
        let method = parts[0];
        let type_name = parts[1];
        let combined = format!("{}.{}", type_name, method);
        let suffix = format!(".{}", combined);
        let candidates: Vec<&GraphNode> = by_name
            .iter()
            .filter(|(qn, _)| **qn == combined.as_str() || qn.ends_with(&suffix))
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        if candidates.len() > 1 {
            // Multiple candidates: prefer one in same package/directory
            if let Some(pkg) = pkg_prefix {
                if let Some(best) = candidates
                    .iter()
                    .find(|n| n.qualified_name.starts_with(pkg) || n.file_path.contains(pkg))
                {
                    return Some(best);
                }
            }
            // Fall back to first candidate
            return Some(candidates[0]);
        }
    }

    // For plain functions (no receiver), try matching just the function name
    // but only when there's a single unambiguous match.
    if let Some(short) = qualified.rsplit('.').next() {
        // Only match by the full qualified_name (not just the short name suffix)
        // to avoid false positives like "Handle" matching unrelated handlers.
        let exact_suffix = format!(".{}", short);
        let candidates: Vec<&GraphNode> = by_name
            .iter()
            .filter(|(qn, _)| **qn == short || qn.ends_with(&exact_suffix))
            .flat_map(|(_, nodes)| nodes.iter().copied())
            .collect();
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        if candidates.len() > 1 {
            // Multiple candidates: prefer same package/directory as caller
            if let Some(pkg) = pkg_prefix {
                if let Some(best) = candidates
                    .iter()
                    .find(|n| n.qualified_name.starts_with(pkg) || n.file_path.contains(pkg))
                {
                    return Some(best);
                }
            }
            // Do NOT fall back to an arbitrary candidate when ambiguous —
            // returning a wrong match produces incorrect call edges.
            return None;
        }
    }
    None
}

/// Extract call graph from a TypeScript/JavaScript repository using typescript-language-server.
pub fn extract_call_graph_typescript(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    if !tsserver_available() {
        return LspCallGraphResult {
            edges: Vec::new(),
            errors: vec![
                "typescript-language-server not found — skipping TypeScript LSP call graph".into(),
            ],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: false,
            missing_toolchains: vec!["typescript-language-server".into()],
        };
    }

    // typescript-language-server handles .ts, .tsx, .js, and .jsx files.
    // Use a single language server instance for all TypeScript/JavaScript files
    // (previously spawned two — doubling extraction time and missing .tsx/.jsx).
    extract_call_graph_via_lsp(
        repo_root,
        nodes,
        existing_edges,
        repo_id,
        commit_sha,
        "typescript-language-server",
        &["--stdio"],
        "typescriptreact",
        &[".ts", ".tsx", ".js", ".jsx"],
    )
}

/// Extract call graph for any supported language, auto-detecting the language.
pub fn extract_call_graph_auto(
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    // Detect ALL languages in polyglot repos and extract from each independently.
    // This ensures a Rust+TypeScript repo (like Gyre) gets call edges from both.
    let languages = detect_all_languages(repo_root);
    if languages.is_empty() {
        return LspCallGraphResult {
            edges: Vec::new(),
            errors: vec!["Unknown language — no LSP call graph extractor available".into()],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: false,
            missing_toolchains: vec![],
        };
    }

    // Single language — fast path (avoid edge deduplication overhead)
    if languages.len() == 1 {
        return extract_call_graph_for_language(
            languages[0],
            repo_root,
            nodes,
            existing_edges,
            repo_id,
            commit_sha,
        );
    }

    // Multiple languages — merge results
    let mut combined = LspCallGraphResult {
        edges: Vec::new(),
        errors: Vec::new(),
        definitions_queried: 0,
        new_edges_found: 0,
        total_definitions: 0,
        incomplete: false,
        missing_toolchains: Vec::new(),
    };

    // Accumulate edges from all existing + previously extracted to avoid duplicates
    let mut all_edges = existing_edges.to_vec();

    for lang in &languages {
        let result = extract_call_graph_for_language(
            *lang, repo_root, nodes, &all_edges, repo_id, commit_sha,
        );
        // Merge into combined edges, extending the edge set for subsequent extractors
        all_edges.extend(result.edges.iter().cloned());
        combined.edges.extend(result.edges);
        combined.errors.extend(result.errors);
        combined.definitions_queried += result.definitions_queried;
        combined.new_edges_found += result.new_edges_found;
        combined.total_definitions += result.total_definitions;
        combined.incomplete |= result.incomplete;
        combined
            .missing_toolchains
            .extend(result.missing_toolchains);
    }

    combined
}

fn extract_call_graph_for_language(
    lang: RepoLanguage,
    repo_root: &Path,
    nodes: &[GraphNode],
    existing_edges: &[GraphEdge],
    repo_id: &Id,
    commit_sha: &str,
) -> LspCallGraphResult {
    match lang {
        RepoLanguage::Rust => {
            extract_call_graph(repo_root, nodes, existing_edges, repo_id, commit_sha)
        }
        RepoLanguage::Python => {
            extract_call_graph_python(repo_root, nodes, existing_edges, repo_id, commit_sha)
        }
        RepoLanguage::Go => {
            extract_call_graph_go(repo_root, nodes, existing_edges, repo_id, commit_sha)
        }
        RepoLanguage::TypeScript => {
            extract_call_graph_typescript(repo_root, nodes, existing_edges, repo_id, commit_sha)
        }
        RepoLanguage::Unknown => LspCallGraphResult {
            edges: Vec::new(),
            errors: vec!["Unknown language — no LSP call graph extractor available".into()],
            definitions_queried: 0,
            new_edges_found: 0,
            total_definitions: 0,
            incomplete: false,
            missing_toolchains: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::*;

    fn make_function_node(
        id: &str,
        name: &str,
        file: &str,
        line_start: u32,
        line_end: u32,
    ) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            node_type: NodeType::Function,
            name: name.to_string(),
            qualified_name: format!("pkg.{}", name),
            file_path: file.to_string(),
            line_start,
            line_end,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc123".to_string(),
            last_modified_by: None,
            last_modified_at: 1000,
            created_sha: "abc123".to_string(),
            created_at: 1000,
            complexity: Some(5),
            churn_count_30d: 2,
            test_coverage: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    #[test]
    fn test_rust_analyzer_availability() {
        let _available = rust_analyzer_available();
    }

    #[test]
    fn test_extract_empty_nodes() {
        let result = extract_call_graph(
            Path::new("/nonexistent"),
            &[],
            &[],
            &Id::new("repo1"),
            "abc123",
        );
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.definitions_queried, 0);
    }

    #[test]
    fn test_enclosing_function_resolution() {
        // Test that we correctly find the enclosing function using line ranges.
        let nodes = vec![
            make_function_node("n1", "outer_fn", "src/lib.rs", 1, 20),
            make_function_node("n2", "inner_fn", "src/lib.rs", 5, 15),
        ];
        let mut file_functions: HashMap<String, Vec<(u32, u32, String)>> = HashMap::new();
        for n in &nodes {
            file_functions
                .entry(n.file_path.clone())
                .or_default()
                .push((n.line_start, n.line_end, n.id.to_string()));
        }

        // A reference on line 10 should resolve to inner_fn (more specific)
        let functions = file_functions.get("src/lib.rs").unwrap();
        let caller = functions
            .iter()
            .filter(|(start, end, _)| *start <= 10 && *end >= 10)
            .max_by_key(|(start, _, _)| *start)
            .map(|(_, _, id)| id.clone());
        assert_eq!(caller.as_deref(), Some("n2")); // inner_fn, not outer_fn

        // A reference on line 18 should resolve to outer_fn
        let caller2 = functions
            .iter()
            .filter(|(start, end, _)| *start <= 18 && *end >= 18)
            .max_by_key(|(start, _, _)| *start)
            .map(|(_, _, id)| id.clone());
        assert_eq!(caller2.as_deref(), Some("n1")); // outer_fn
    }

    // ── compute_char_position tests ───────────────────────────────────────

    #[test]
    fn char_position_plain_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn hello() {}\n").unwrap();
        let node = make_function_node("n1", "hello", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 3);
    }

    #[test]
    fn char_position_pub_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();
        let node = make_function_node("n1", "hello", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 7); // "pub fn " = 7
    }

    #[test]
    fn char_position_pub_async_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub async fn serve() {}\n").unwrap();
        let node = make_function_node("n1", "serve", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 13); // "pub async fn " = 13 chars to name
    }

    #[test]
    fn char_position_pub_crate_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub(crate) fn internal() {}\n").unwrap();
        let node = make_function_node("n1", "internal", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 14);
    }

    #[test]
    fn char_position_const_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "const fn max() -> u32 { 42 }\n").unwrap();
        let node = make_function_node("n1", "max", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 9);
    }

    #[test]
    fn char_position_unsafe_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "unsafe fn danger() {}\n").unwrap();
        let node = make_function_node("n1", "danger", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 10);
    }

    #[test]
    fn char_position_extern_c_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("lib.rs"),
            "pub unsafe extern \"C\" fn init() {}\n",
        )
        .unwrap();
        let node = make_function_node("n1", "init", "lib.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "lib.rs", &node), 25);
    }

    #[test]
    fn char_position_does_not_match_prefix() {
        // "fn foo" should NOT match "fn foo_bar" — word-boundary matching prevents
        // "foo" from matching inside "foo_bar"
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn foo_bar() {}\n").unwrap();
        let node = make_function_node("n1", "foo", "lib.rs", 1, 1);
        let pos = compute_char_position(dir.path(), "lib.rs", &node);
        // Word-boundary check correctly rejects "foo" inside "foo_bar", returns 0
        assert_eq!(pos, 0);
    }

    #[test]
    fn char_position_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let node = make_function_node("n1", "missing", "missing.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "missing.rs", &node), 0);
    }

    // ── Multi-language detection tests ───────────────────────────────

    #[test]
    fn detect_rust_by_cargo_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        assert_eq!(detect_language(dir.path()), RepoLanguage::Rust);
    }

    #[test]
    fn detect_python_by_pyproject() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();
        assert_eq!(detect_language(dir.path()), RepoLanguage::Python);
    }

    #[test]
    fn detect_go_by_go_mod() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test\ngo 1.21").unwrap();
        assert_eq!(detect_language(dir.path()), RepoLanguage::Go);
    }

    #[test]
    fn detect_typescript_by_tsconfig() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        assert_eq!(detect_language(dir.path()), RepoLanguage::TypeScript);
    }

    #[test]
    fn detect_unknown_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(detect_language(dir.path()), RepoLanguage::Unknown);
    }

    #[test]
    fn language_tool_availability_checks() {
        // These just exercise the availability checks — they may return
        // true or false depending on the environment, but should not panic.
        let _ = pyright_available();
        let _ = gopls_available();
        let _ = tsserver_available();
    }

    #[test]
    fn extract_auto_unknown_language() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = extract_call_graph_auto(dir.path(), &[], &[], &Id::new("repo1"), "abc123");
        assert_eq!(result.edges.len(), 0);
        assert!(result.errors[0].contains("Unknown language"));
    }

    #[test]
    fn extract_python_empty_nodes() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let result = extract_call_graph_python(dir.path(), &[], &[], &Id::new("repo1"), "abc123");
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.definitions_queried, 0);
    }

    #[test]
    fn extract_go_empty_nodes() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test").unwrap();
        let result = extract_call_graph_go(dir.path(), &[], &[], &Id::new("repo1"), "abc123");
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.definitions_queried, 0);
    }

    #[test]
    fn extract_typescript_empty_nodes() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let result =
            extract_call_graph_typescript(dir.path(), &[], &[], &Id::new("repo1"), "abc123");
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.definitions_queried, 0);
    }
}
