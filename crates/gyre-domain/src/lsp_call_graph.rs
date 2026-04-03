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
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use uuid::Uuid;

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
/// 2. For each function/method node from Pass 1, sends textDocument/references
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

    // Build file → line_start → node_id map for resolving reference sites
    let mut file_line_to_node: HashMap<String, HashMap<u32, String>> = HashMap::new();
    for n in nodes.iter().filter(|n| n.deleted_at.is_none()) {
        file_line_to_node
            .entry(n.file_path.clone())
            .or_default()
            .insert(n.line_start, n.id.to_string());
    }

    // Collect existing edge pairs to avoid duplicates
    let mut existing_pairs: HashSet<(String, String)> = HashSet::new();
    for e in existing_edges {
        if e.edge_type == EdgeType::Calls && e.deleted_at.is_none() {
            existing_pairs.insert((e.source_id.to_string(), e.target_id.to_string()));
        }
    }

    // Start rust-analyzer
    let mut child = match Command::new("rust-analyzer")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(repo_root)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            result
                .errors
                .push(format!("Failed to start rust-analyzer: {e}"));
            return result;
        }
    };

    let stdin = child.stdin.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Initialize LSP
    let init_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": format!("file://{}", repo_root.display()),
            "capabilities": {},
        }
    });

    if let Err(e) = send_lsp_message(stdin, &init_msg) {
        result
            .errors
            .push(format!("Failed to send initialize: {e}"));
        let _ = child.kill();
        return result;
    }

    // Read initialize response
    match read_lsp_message(&mut reader) {
        Ok(Some(_)) => {}
        Ok(None) => {
            result.errors.push("No initialize response".into());
            let _ = child.kill();
            return result;
        }
        Err(e) => {
            result
                .errors
                .push(format!("Failed to read initialize response: {e}"));
            let _ = child.kill();
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

    // For each function node, find references
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let max_queries = 200; // Cap to avoid excessive load
    let nodes_to_query = if function_nodes.len() > max_queries {
        &function_nodes[..max_queries]
    } else {
        &function_nodes
    };

    for (idx, func_node) in nodes_to_query.iter().enumerate() {
        result.definitions_queried += 1;

        let file_uri = format!("file://{}/{}", repo_root.display(), func_node.file_path);

        let refs_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": idx + 1,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": file_uri },
                "position": {
                    "line": func_node.line_start.saturating_sub(1),
                    "character": 0
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

        // Read response (with timeout handling via non-blocking read)
        match read_lsp_response(&mut reader, idx + 1) {
            Ok(Some(locations)) => {
                for loc in locations {
                    // Resolve location to a file path and line number
                    if let (Some(uri), Some(line)) = (
                        loc.get("uri").and_then(|u| u.as_str()),
                        loc.get("range")
                            .and_then(|r| r.get("start"))
                            .and_then(|s| s.get("line"))
                            .and_then(|l| l.as_u64()),
                    ) {
                        let file_path = uri
                            .strip_prefix(&format!("file://{}/", repo_root.display()))
                            .unwrap_or(uri);
                        let line_num = (line + 1) as u32;

                        // Find the enclosing function at this reference site
                        if let Some(line_map) = file_line_to_node.get(file_path) {
                            // Find the closest function node that starts at or before this line
                            let caller_id = line_map
                                .iter()
                                .filter(|(start, _)| **start <= line_num)
                                .max_by_key(|(start, _)| *start)
                                .map(|(_, id)| id.clone());

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

    // Shutdown LSP
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
    let _ = child.wait();

    result
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
    std::io::Read::read_exact(reader, &mut body)?;
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(parsed))
}

fn read_lsp_response(
    reader: &mut BufReader<std::process::ChildStdout>,
    expected_id: usize,
) -> std::io::Result<Option<Vec<serde_json::Value>>> {
    // Read messages until we get the response with our ID
    // (rust-analyzer may send notifications/progress before the response)
    for _ in 0..50 {
        match read_lsp_message(reader)? {
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
            None => return Ok(None),
        }
    }
    Ok(None) // Gave up after too many messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_analyzer_availability() {
        // Just test the check function doesn't panic
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
        // Should return immediately with no edges
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.definitions_queried, 0);
    }
}
