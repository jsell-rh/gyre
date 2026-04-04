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
    _commit_sha: &str,
) -> LspCallGraphResult {
    let mut result = LspCallGraphResult {
        edges: Vec::new(),
        errors: Vec::new(),
        definitions_queried: 0,
        new_edges_found: 0,
    };

    if !rust_analyzer_available() {
        result
            .errors
            .push("rust-analyzer not found on PATH — skipping LSP call graph extraction".into());
        return result;
    }

    // Build lookup maps
    let function_nodes: Vec<&GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.deleted_at.is_none() && matches!(n.node_type, NodeType::Function | NodeType::Endpoint)
        })
        .collect();

    if function_nodes.is_empty() {
        return result;
    }

    // Build file → sorted vec of (line_start, line_end, node_id) for resolving reference sites.
    let mut file_functions: HashMap<String, Vec<(u32, u32, String)>> = HashMap::new();
    for n in nodes.iter().filter(|n| n.deleted_at.is_none()) {
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
    // rust-analyzer sends `$/progress` notifications during indexing;
    // we wait until we see a `kind: "end"` value or hit a 30-second timeout.
    let index_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() > index_deadline {
            eprintln!("rust-analyzer indexing timeout after 30s, proceeding anyway");
            break;
        }
        match read_lsp_message_with_timeout(&mut reader, Duration::from_millis(500)) {
            Ok(Some(msg)) => {
                if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                    if method == "$/progress" {
                        if let Some(value) = msg.get("params").and_then(|p| p.get("value")) {
                            if value.get("kind").and_then(|k| k.as_str()) == Some("end") {
                                eprintln!("rust-analyzer indexing complete");
                                break;
                            }
                        }
                    }
                }
            }
            Ok(None) | Err(_) => continue, // Timeout or empty message, keep waiting
        }
    }

    // Track opened files for didOpen notifications.
    let mut opened_files: HashSet<String> = HashSet::new();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // No artificial cap — query all function nodes. For large repos
    // (~1800 nodes), this completes in ~20 seconds per the spec estimate.
    for (idx, func_node) in function_nodes.iter().enumerate() {
        // Check overall deadline to prevent runaway extraction.
        if Instant::now() > overall_deadline {
            result.errors.push(format!(
                "Overall extraction timeout after {} definitions",
                result.definitions_queried
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
                                    result.edges.push(GraphEdge {
                                        id: Id::new(Uuid::new_v4().to_string()),
                                        repo_id: repo_id.clone(),
                                        source_id: Id::new(caller),
                                        target_id: func_node.id.clone(),
                                        edge_type: EdgeType::Calls,
                                        metadata: Some(r#"{"source":"lsp"}"#.to_string()),
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

    result
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

    // Primary: find "fn <name>" anywhere in the line.  This correctly handles
    // all modifier combinations (pub, pub(crate), async, const, unsafe,
    // extern "C", etc.) because `find` matches the substring regardless of
    // what precedes it.
    let needle = format!("fn {}", node.name);
    if let Some(pos) = line.find(&needle) {
        // Verify we matched the exact name, not a prefix (e.g. "fn foo" vs "fn foo_bar").
        let after = pos + needle.len();
        let next_char = line.as_bytes().get(after).copied();
        let is_exact = match next_char {
            None => true,                                       // end of line
            Some(c) => !c.is_ascii_alphanumeric() && c != b'_', // delimiter follows
        };
        if is_exact {
            return (pos + 3) as u32; // "fn " = 3 chars, position at name start
        }
    }

    // Fallback: use the LAST occurrence of the name in the line.  Using rfind
    // avoids matching an earlier occurrence that might be part of a type
    // annotation or attribute rather than the actual definition name.
    if let Some(pos) = line.rfind(&node.name) {
        return pos as u32;
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

    // If the BufReader already has buffered data, skip the poll and read directly.
    if !reader.buffer().is_empty() {
        return read_lsp_message(reader);
    }

    // Use poll(2) to wait for data on the stdout fd with a timeout.
    let fd = reader.get_ref().as_raw_fd();
    let timeout_ms = timeout.as_millis() as i32;

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
        // "fn foo" should NOT match "fn foo_bar"
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn foo_bar() {}\n").unwrap();
        let node = make_function_node("n1", "foo", "lib.rs", 1, 1);
        // Should fall through to rfind("foo") which finds position 3
        // (inside "foo_bar"), but that's the best we can do with rfind.
        let pos = compute_char_position(dir.path(), "lib.rs", &node);
        // rfind("foo") in "fn foo_bar() {}" finds index 3 (the "foo" in "foo_bar")
        assert_eq!(pos, 3);
    }

    #[test]
    fn char_position_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let node = make_function_node("n1", "missing", "missing.rs", 1, 1);
        assert_eq!(compute_char_position(dir.path(), "missing.rs", &node), 0);
    }
}
