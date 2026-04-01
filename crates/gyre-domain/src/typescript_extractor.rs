//! TypeScript/JavaScript language extractor for the knowledge graph.
//!
//! Walks a TypeScript/JavaScript repository and extracts:
//! - Modules (.ts, .tsx, .js, .jsx files)
//! - Classes (`class_declaration`)
//! - Interfaces (`interface_declaration`)
//! - Exported functions (`export function` / `export const f = () =>`)
//! - API call sites (`fetch("/path")` / `axios.get("/path")`) → Endpoint nodes + Calls edges
//!
//! Only exported symbols are extracted; non-exported symbols are internal details.

use crate::extractor::{ExtractionError, ExtractionResult, LanguageExtractor};
use crate::tree_sitter_utils::parse_source;
use gyre_common::{
    graph::{EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence, Visibility},
    Id,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use walkdir::WalkDir;

/// TypeScript/JavaScript language extractor.
///
/// Detects repositories with a `package.json` at the root and walks all
/// `.ts`, `.tsx`, `.js`, and `.jsx` files to extract architectural knowledge.
pub struct TypeScriptExtractor;

/// Directories to skip during file traversal.
const SKIP_DIRS: &[&str] = &["node_modules", "dist", ".next", "build"];

impl LanguageExtractor for TypeScriptExtractor {
    fn name(&self) -> &str {
        "typescript"
    }

    fn detect(&self, repo_root: &Path) -> bool {
        repo_root.join("package.json").is_file()
    }

    fn extract(&self, repo_root: &Path, commit_sha: &str) -> ExtractionResult {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut ctx = ExtractionContext {
            repo_root: repo_root.to_path_buf(),
            commit_sha: commit_sha.to_string(),
            now,
            nodes: Vec::new(),
            edges: Vec::new(),
            errors: Vec::new(),
            name_to_id: HashMap::new(),
        };

        // Pass 1: tree-sitter based extraction (declarations + API call sites)
        ctx.extract_ts_files();

        // Pass 2: TypeScript compiler API call-graph extraction (type-resolved)
        ctx.extract_lsp_call_edges();

        ExtractionResult {
            nodes: ctx.nodes,
            edges: ctx.edges,
            errors: ctx.errors,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal extraction context
// ---------------------------------------------------------------------------

struct ExtractionContext {
    repo_root: PathBuf,
    commit_sha: String,
    now: u64,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    errors: Vec<ExtractionError>,
    /// Map qualified_name → node Id for edge resolution and deduplication.
    name_to_id: HashMap<String, Id>,
}

impl ExtractionContext {
    fn new_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
    }

    /// Placeholder repo_id — callers set the real repo_id when persisting nodes.
    fn placeholder_repo_id() -> Id {
        Id::new(String::new())
    }

    #[allow(clippy::too_many_arguments)]
    fn make_node(
        &self,
        node_type: NodeType,
        name: &str,
        qualified_name: &str,
        file_path: &str,
        line_start: u32,
        line_end: u32,
        visibility: Visibility,
    ) -> GraphNode {
        GraphNode {
            id: Self::new_id(),
            repo_id: Self::placeholder_repo_id(),
            node_type,
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            file_path: file_path.to_string(),
            line_start,
            line_end,
            visibility,
            doc_comment: None,
            spec_path: None,
            spec_confidence: SpecConfidence::None,
            last_modified_sha: self.commit_sha.clone(),
            last_modified_by: None,
            last_modified_at: self.now,
            created_sha: self.commit_sha.clone(),
            created_at: self.now,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            // Incremental diffing in graph_extraction.rs sets these from the prior state.
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }
    }

    fn make_edge(&self, edge_type: EdgeType, source_id: Id, target_id: Id) -> GraphEdge {
        GraphEdge {
            id: Self::new_id(),
            repo_id: Self::placeholder_repo_id(),
            source_id,
            target_id,
            edge_type,
            metadata: None,
            // Incremental diffing in graph_extraction.rs sets these from the prior state.
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }
    }

    // -----------------------------------------------------------------------
    // Pass 2: LSP-powered call graph via ts-callgraph.mjs
    // -----------------------------------------------------------------------

    /// Shell out to `scripts/ts-callgraph.mjs` to extract type-resolved call
    /// edges using the TypeScript compiler API.  Falls back gracefully if
    /// `node` is not available or the script is missing.
    fn extract_lsp_call_edges(&mut self) {
        let script_path = find_callgraph_script();
        let script_path = match script_path {
            Some(p) => p,
            None => return, // script not found — degrade gracefully
        };

        let output = match Command::new("node")
            .arg(&script_path)
            .arg(&self.repo_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => return, // node not available
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                self.errors.push(ExtractionError {
                    file_path: script_path.display().to_string(),
                    message: format!("ts-callgraph.mjs failed: {stderr}"),
                    line: None,
                });
            }
            return;
        }

        let stdout = match std::str::from_utf8(&output.stdout) {
            Ok(s) => s.trim(),
            Err(_) => return,
        };

        let call_edges: Vec<CallGraphEdge> = match serde_json::from_str(stdout) {
            Ok(v) => v,
            Err(e) => {
                self.errors.push(ExtractionError {
                    file_path: script_path.display().to_string(),
                    message: format!("ts-callgraph.mjs JSON parse error: {e}"),
                    line: None,
                });
                return;
            }
        };

        // Build a set of existing Calls edges for deduplication.
        let existing_calls: HashSet<(String, String)> = self
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .filter_map(|e| {
                let src = self.id_to_qname(&e.source_id)?;
                let tgt = self.id_to_qname(&e.target_id)?;
                Some((src, tgt))
            })
            .collect();

        for ce in call_edges {
            // Skip if already covered by Pass 1
            if existing_calls.contains(&(ce.from.clone(), ce.to.clone())) {
                continue;
            }

            let source_id = self.ensure_node_for_qname(&ce.from);
            let target_id = self.ensure_node_for_qname(&ce.to);

            let edge = self.make_edge(EdgeType::Calls, source_id, target_id);
            self.edges.push(edge);
        }
    }

    /// Reverse-lookup: given an Id, find the qualified_name.
    fn id_to_qname(&self, id: &Id) -> Option<String> {
        self.name_to_id
            .iter()
            .find(|(_, v)| *v == id)
            .map(|(k, _)| k.clone())
    }

    /// Get or create a graph node for a qualified name.  If the name already
    /// exists in `name_to_id`, return the existing Id.  Otherwise create a
    /// minimal Function node so that the Calls edge has valid endpoints.
    fn ensure_node_for_qname(&mut self, qname: &str) -> Id {
        if let Some(id) = self.name_to_id.get(qname) {
            return id.clone();
        }

        // Derive a short name and file path from the qualified name.
        let short_name = qname.rsplit('.').next().unwrap_or(qname);
        // The module part is everything before the last dot, with `/` separators.
        let file_hint = if let Some(dot_pos) = qname.rfind('.') {
            &qname[..dot_pos]
        } else {
            qname
        };

        let node = self.make_node(
            NodeType::Function,
            short_name,
            qname,
            file_hint,
            0,
            0,
            Visibility::Public,
        );
        let id = node.id.clone();
        self.name_to_id.insert(qname.to_string(), id.clone());
        self.nodes.push(node);
        id
    }

    // -----------------------------------------------------------------------
    // File discovery
    // -----------------------------------------------------------------------

    fn extract_ts_files(&mut self) {
        let ts_files: Vec<PathBuf> = WalkDir::new(&self.repo_root)
            .into_iter()
            .filter_entry(|e| {
                // Skip known build/vendor directories at any depth.
                if e.file_type().is_dir() {
                    let name = e.file_name().to_str().unwrap_or("");
                    return !SKIP_DIRS.contains(&name);
                }
                true
            })
            .filter_map(|e| e.ok())
            .filter(|e| is_ts_extension(e.path()))
            .map(|e| e.into_path())
            .collect();

        for path in ts_files {
            if let Err(e) = self.extract_ts_file(&path) {
                self.errors.push(ExtractionError {
                    file_path: path.display().to_string(),
                    message: e,
                    line: None,
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Single-file extraction
    // -----------------------------------------------------------------------

    fn extract_ts_file(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;
        let source = content.as_bytes();

        let rel_path = path
            .strip_prefix(&self.repo_root)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        // Module qualified_name: path without extension (keep "/" separators).
        let module_qname = module_qname_from_path(&rel_path);
        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        // Emit Module node.
        let module_node = self.make_node(
            NodeType::Module,
            &module_name,
            &module_qname,
            &rel_path,
            1,
            0,
            Visibility::Public,
        );
        let module_id = module_node.id.clone();
        self.name_to_id
            .insert(module_qname.clone(), module_id.clone());
        self.nodes.push(module_node);

        // Choose grammar: .tsx uses the TSX grammar, everything else uses TS.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let language = if ext == "tsx" {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };

        let tree = parse_source(source, language)?;
        let root = tree.root_node();

        // --- Pass 1: top-level declarations (classes, interfaces, exports) ---
        for i in 0..root.child_count() {
            let Some(child) = root.child(i) else {
                continue;
            };
            match child.kind() {
                "class_declaration" => {
                    self.emit_class(&content, child, &rel_path, &module_qname, &module_id);
                }
                "interface_declaration" => {
                    self.emit_interface(&content, child, &rel_path, &module_qname, &module_id);
                }
                "export_statement" => {
                    self.emit_export(&content, child, &rel_path, &module_qname, &module_id);
                }
                _ => {}
            }
        }

        // --- Pass 2: full-tree walk for fetch/axios call sites ---------------
        let api_calls = collect_api_calls(&content, root);
        for (api_path, line) in api_calls {
            let endpoint_name = api_path
                .replace('/', "_")
                .trim_start_matches('_')
                .to_string();
            let endpoint_qname = format!("endpoint:{api_path}");

            let endpoint_id = if let Some(id) = self.name_to_id.get(&endpoint_qname) {
                id.clone()
            } else {
                let ep_node = self.make_node(
                    NodeType::Endpoint,
                    &endpoint_name,
                    &endpoint_qname,
                    &rel_path,
                    line,
                    line,
                    Visibility::Public,
                );
                let ep_id = ep_node.id.clone();
                self.name_to_id.insert(endpoint_qname, ep_id.clone());
                self.nodes.push(ep_node);
                ep_id
            };

            // Calls edge: module → endpoint
            let edge = self.make_edge(EdgeType::Calls, module_id.clone(), endpoint_id);
            self.edges.push(edge);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Node emitters
    // -----------------------------------------------------------------------

    fn emit_class(
        &mut self,
        content: &str,
        node: tree_sitter::Node,
        rel_path: &str,
        module_qname: &str,
        module_id: &Id,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = &content[name_node.byte_range()];
        let qname = format!("{module_qname}.{name}");
        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        let graph_node = self.make_node(
            NodeType::Type,
            name,
            &qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        let node_id = graph_node.id.clone();
        self.name_to_id.insert(qname, node_id.clone());
        let edge = self.make_edge(EdgeType::Contains, module_id.clone(), node_id);
        self.nodes.push(graph_node);
        self.edges.push(edge);
    }

    fn emit_interface(
        &mut self,
        content: &str,
        node: tree_sitter::Node,
        rel_path: &str,
        module_qname: &str,
        module_id: &Id,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = &content[name_node.byte_range()];
        let qname = format!("{module_qname}.{name}");
        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        let graph_node = self.make_node(
            NodeType::Interface,
            name,
            &qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        let node_id = graph_node.id.clone();
        self.name_to_id.insert(qname, node_id.clone());
        let edge = self.make_edge(EdgeType::Contains, module_id.clone(), node_id);
        self.nodes.push(graph_node);
        self.edges.push(edge);
    }

    fn emit_export(
        &mut self,
        content: &str,
        export_node: tree_sitter::Node,
        rel_path: &str,
        module_qname: &str,
        module_id: &Id,
    ) {
        // Walk children of the export_statement to find the exported declaration.
        for i in 0..export_node.child_count() {
            let Some(child) = export_node.child(i) else {
                continue;
            };
            match child.kind() {
                "function_declaration" => {
                    let Some(name_node) = child.child_by_field_name("name") else {
                        continue;
                    };
                    let name = &content[name_node.byte_range()];
                    let qname = format!("{module_qname}.{name}");
                    let line_start = child.start_position().row as u32 + 1;
                    let line_end = child.end_position().row as u32 + 1;

                    let graph_node = self.make_node(
                        NodeType::Function,
                        name,
                        &qname,
                        rel_path,
                        line_start,
                        line_end,
                        Visibility::Public,
                    );
                    let node_id = graph_node.id.clone();
                    self.name_to_id.insert(qname, node_id.clone());
                    let edge = self.make_edge(EdgeType::Contains, module_id.clone(), node_id);
                    self.nodes.push(graph_node);
                    self.edges.push(edge);
                }
                "lexical_declaration" => {
                    // `export const funcName = () => ...` or `export const funcName = function() ...`
                    for j in 0..child.child_count() {
                        let Some(decl) = child.child(j) else {
                            continue;
                        };
                        if decl.kind() != "variable_declarator" {
                            continue;
                        }
                        let Some(name_node) = decl.child_by_field_name("name") else {
                            continue;
                        };
                        let value_is_fn = decl
                            .child_by_field_name("value")
                            .map(|v| matches!(v.kind(), "arrow_function" | "function_expression"))
                            .unwrap_or(false);
                        if value_is_fn {
                            let name = &content[name_node.byte_range()];
                            let qname = format!("{module_qname}.{name}");
                            let line_start = decl.start_position().row as u32 + 1;
                            let line_end = decl.end_position().row as u32 + 1;

                            let graph_node = self.make_node(
                                NodeType::Function,
                                name,
                                &qname,
                                rel_path,
                                line_start,
                                line_end,
                                Visibility::Public,
                            );
                            let node_id = graph_node.id.clone();
                            self.name_to_id.insert(qname, node_id.clone());
                            let edge =
                                self.make_edge(EdgeType::Contains, module_id.clone(), node_id);
                            self.nodes.push(graph_node);
                            self.edges.push(edge);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pure tree traversal helpers (no mutation — collect then process)
// ---------------------------------------------------------------------------

/// Recursively collect `(api_path, line_number)` for all `fetch`/`axios.*` call
/// sites in the tree.  Returns an empty vec when no API calls are found.
fn collect_api_calls(content: &str, node: tree_sitter::Node) -> Vec<(String, u32)> {
    let mut results = Vec::new();
    collect_api_calls_inner(content, node, &mut results);
    results
}

fn collect_api_calls_inner(
    content: &str,
    node: tree_sitter::Node,
    results: &mut Vec<(String, u32)>,
) {
    if node.kind() == "call_expression" {
        if let Some(path) = try_extract_api_path(content, node) {
            let line = node.start_position().row as u32 + 1;
            results.push((path, line));
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_api_calls_inner(content, child, results);
        }
    }
}

/// Try to extract an API path string from a `call_expression` node.
///
/// Matches:
/// - `fetch("/path/...")`
/// - `axios.get("/path/...")`, `axios.post(...)`, etc.
///
/// Returns `Some(path)` only when the first argument is a string literal
/// starting with `/`.
fn try_extract_api_path(content: &str, call_node: tree_sitter::Node) -> Option<String> {
    let function_node = call_node.child_by_field_name("function")?;
    let callee = &content[function_node.byte_range()];

    let is_fetch = callee == "fetch";
    let is_axios = callee.starts_with("axios.");
    if !is_fetch && !is_axios {
        return None;
    }

    // First named child of `arguments` is the URL argument.
    let args_node = call_node.child_by_field_name("arguments")?;
    // arguments node children: '(' , arg1, ',' , arg2, ..., ')'
    // Use named_child to skip punctuation.
    let mut cursor = args_node.walk();
    let first_arg = args_node.named_children(&mut cursor).next()?;

    extract_string_literal(content, first_arg).filter(|p| p.starts_with('/'))
}

/// Extract the string value from a `string` node (removing surrounding quotes).
fn extract_string_literal(content: &str, node: tree_sitter::Node) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let raw = &content[node.byte_range()];
    // Remove wrapping `"..."` or `'...'`
    let trimmed = raw.trim_matches(|c| c == '"' || c == '\'');
    Some(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn is_ts_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx")
    )
}

/// Derive module qualified_name from a relative file path.
///
/// Strips the file extension and keeps the path with `/` separators.
/// e.g. `src/components/UserCard.tsx` → `src/components/UserCard`
fn module_qname_from_path(rel_path: &str) -> String {
    if let Some(dot) = rel_path.rfind('.') {
        rel_path[..dot].to_string()
    } else {
        rel_path.to_string()
    }
}

// ---------------------------------------------------------------------------
// LSP call-graph helpers
// ---------------------------------------------------------------------------

/// A single caller→callee edge from the ts-callgraph.mjs script.
#[derive(serde::Deserialize)]
struct CallGraphEdge {
    from: String,
    to: String,
    #[allow(dead_code)]
    line: u32,
}

/// Locate `scripts/ts-callgraph.mjs` relative to the crate/workspace root.
/// Tries several strategies:
/// 1. GYRE_ROOT env var
/// 2. Walking up from the current executable
/// 3. Walking up from the current working directory
fn find_callgraph_script() -> Option<PathBuf> {
    let script_name = "scripts/ts-callgraph.mjs";

    // Strategy 1: explicit env var
    if let Ok(root) = std::env::var("GYRE_ROOT") {
        let candidate = PathBuf::from(root).join(script_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Strategy 2: walk up from current exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = walk_up_for_script(&exe, script_name) {
            return Some(path);
        }
    }

    // Strategy 3: walk up from cwd
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(path) = walk_up_for_script(&cwd, script_name) {
            return Some(path);
        }
    }

    None
}

fn walk_up_for_script(start: &Path, script_name: &str) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = dir.join(script_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::{EdgeType, NodeType};
    use std::fs;
    use tempfile::TempDir;

    fn make_tempdir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// Set up a temp dir with `package.json` and a single source file.
    fn extract_ts(dir: &TempDir, filename: &str, code: &str) -> ExtractionResult {
        fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        fs::write(dir.path().join(filename), code).unwrap();
        TypeScriptExtractor.extract(dir.path(), "abc123")
    }

    #[test]
    fn detect_returns_true_with_package_json() {
        let dir = make_tempdir();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(TypeScriptExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_false_without_package_json() {
        let dir = make_tempdir();
        assert!(!TypeScriptExtractor.detect(dir.path()));
    }

    #[test]
    fn extract_class_from_ts_file() {
        let dir = make_tempdir();
        let result = extract_ts(&dir, "user.ts", "class UserService {\n  greet() {}\n}\n");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let type_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Type && n.name == "UserService");
        assert!(
            type_node.is_some(),
            "should extract UserService class as Type node"
        );
        let qname = &type_node.unwrap().qualified_name;
        assert!(
            qname.ends_with(".UserService"),
            "qualified_name should end with .UserService, got: {qname}"
        );
    }

    #[test]
    fn extract_interface_from_ts_file() {
        let dir = make_tempdir();
        let result = extract_ts(
            &dir,
            "types.ts",
            "interface UserProfile {\n  name: string;\n}\n",
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let iface = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Interface && n.name == "UserProfile");
        assert!(iface.is_some(), "should extract UserProfile interface");
        let qname = &iface.unwrap().qualified_name;
        assert!(
            qname.ends_with(".UserProfile"),
            "qualified_name should end with .UserProfile, got: {qname}"
        );
    }

    #[test]
    fn extract_exported_function() {
        let dir = make_tempdir();
        let result = extract_ts(
            &dir,
            "api.ts",
            "export function fetchUser(id: string) {\n  return id;\n}\n",
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let func = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "fetchUser");
        assert!(func.is_some(), "should extract exported fetchUser function");
    }

    #[test]
    fn extract_non_exported_function_skipped() {
        let dir = make_tempdir();
        let result = extract_ts(&dir, "util.ts", "function helper() {\n  return 42;\n}\n");
        let func = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "helper");
        assert!(func.is_none(), "non-exported function should be skipped");
    }

    #[test]
    fn extract_fetch_call_as_endpoint_edge() {
        let dir = make_tempdir();
        let result = extract_ts(
            &dir,
            "client.ts",
            "async function loadUsers() {\n  return fetch(\"/api/v1/users\");\n}\n",
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let endpoint = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint);
        assert!(
            endpoint.is_some(),
            "should create Endpoint node for fetch(\"/api/v1/users\")"
        );
        let ep = endpoint.unwrap();
        assert!(
            ep.qualified_name.contains("api/v1/users"),
            "endpoint qualified_name should contain path, got: {}",
            ep.qualified_name
        );
        let calls_edge = result.edges.iter().find(|e| e.edge_type == EdgeType::Calls);
        assert!(
            calls_edge.is_some(),
            "should create Calls edge from module to endpoint"
        );
    }

    #[test]
    fn node_modules_skipped() {
        let dir = make_tempdir();
        fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        let nm = dir.path().join("node_modules").join("some-lib");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.ts"), "export class LibClass {}\n").unwrap();
        fs::write(dir.path().join("main.ts"), "export function main() {}\n").unwrap();

        let result = TypeScriptExtractor.extract(dir.path(), "abc123");

        let lib_class = result.nodes.iter().find(|n| n.name == "LibClass");
        assert!(lib_class.is_none(), "node_modules/ files should be skipped");

        let main_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "main");
        assert!(
            main_fn.is_some(),
            "main.ts exports should still be extracted"
        );
    }

    #[test]
    fn extract_exported_arrow_function() {
        let dir = make_tempdir();
        let result = extract_ts(
            &dir,
            "handlers.ts",
            "export const getUser = (id: string) => id;\n",
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let func = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "getUser");
        assert!(
            func.is_some(),
            "should extract exported arrow function getUser"
        );
    }

    /// Multi-file test: Pass 2 (LSP) should produce cross-file Calls edges
    /// when one module imports and calls a function from another.
    #[test]
    fn lsp_cross_file_call_edges() {
        // Skip if node is not available
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping lsp_cross_file_call_edges: node not found");
            return;
        }

        // Skip if the call-graph script can't be found
        if find_callgraph_script().is_none() {
            eprintln!("skipping lsp_cross_file_call_edges: ts-callgraph.mjs not found");
            return;
        }

        let dir = make_tempdir();
        fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

        // Module A: exports a helper
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("math.ts"),
            "export function add(a: number, b: number): number {\n  return a + b;\n}\n\nexport function multiply(a: number, b: number): number {\n  return a * b;\n}\n",
        )
        .unwrap();

        // Module B: imports and calls the helper
        fs::write(
            src.join("calc.ts"),
            "import { add, multiply } from './math';\n\nexport function calculate(x: number): number {\n  return add(x, multiply(x, 2));\n}\n",
        )
        .unwrap();

        let result = TypeScriptExtractor.extract(dir.path(), "abc123");

        // Filter errors (ignore non-fatal ones from ts-callgraph)
        let fatal_errors: Vec<_> = result
            .errors
            .iter()
            .filter(|e| !e.message.contains("ts-callgraph"))
            .collect();
        assert!(
            fatal_errors.is_empty(),
            "unexpected errors: {:?}",
            fatal_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // There should be Calls edges from calc → math.add and calc → math.multiply
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();

        // Look up node names by ID for readable assertions
        let node_name = |id: &Id| -> String {
            result
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .map(|n| n.qualified_name.clone())
                .unwrap_or_else(|| format!("unknown({id})"))
        };

        let cross_file_calls: Vec<_> = calls_edges
            .iter()
            .filter(|e| {
                let src = node_name(&e.source_id);
                let tgt = node_name(&e.target_id);
                // Cross-file: source contains "calc", target contains "math"
                src.contains("calc") && tgt.contains("math")
            })
            .collect();

        assert!(
            cross_file_calls.len() >= 2,
            "expected at least 2 cross-file call edges (calc->math.add, calc->math.multiply), \
             found {}. All calls edges: {:?}",
            cross_file_calls.len(),
            calls_edges
                .iter()
                .map(|e| format!("{} -> {}", node_name(&e.source_id), node_name(&e.target_id)))
                .collect::<Vec<_>>()
        );
    }

    /// Verify that Pass 2 degrades gracefully when node is not available
    /// (e.g. by setting GYRE_ROOT to a nonexistent path so the script isn't found).
    #[test]
    fn lsp_graceful_degradation_without_script() {
        let dir = make_tempdir();
        fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        fs::write(
            dir.path().join("app.ts"),
            "export function hello() { return 'hi'; }\n",
        )
        .unwrap();

        // Set GYRE_ROOT to a temp dir that won't have the script
        let fake_root = make_tempdir();
        std::env::set_var("GYRE_ROOT", fake_root.path());
        let result = TypeScriptExtractor.extract(dir.path(), "abc123");
        std::env::remove_var("GYRE_ROOT");

        // Should still produce Pass 1 results without errors
        let func = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "hello");
        assert!(
            func.is_some(),
            "Pass 1 extraction should still work when script is missing"
        );
    }
}
