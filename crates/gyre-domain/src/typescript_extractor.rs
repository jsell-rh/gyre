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

        ctx.extract_ts_files();

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

        // --- Pass 3: general function-to-function Calls edges ---
        self.extract_fn_calls(&content, root, &module_qname);

        Ok(())
    }

    fn extract_fn_calls(&mut self, content: &str, root: tree_sitter::Node, module_qname: &str) {
        let mut fn_calls: Vec<(Id, String)> = Vec::new();
        self.collect_fn_calls(content, root, module_qname, None, &mut fn_calls);

        let mut seen = HashSet::new();
        for (from_id, callee_name) in fn_calls {
            if let Some(to_id) = self.resolve_ts_callee(&callee_name, module_qname) {
                if from_id != to_id {
                    let key = (from_id.to_string(), to_id.to_string());
                    if seen.insert(key) {
                        let edge = self.make_edge(EdgeType::Calls, from_id, to_id);
                        self.edges.push(edge);
                    }
                }
            }
        }
    }

    fn collect_fn_calls(
        &self,
        content: &str,
        node: tree_sitter::Node,
        module_qname: &str,
        current_fn_id: Option<&Id>,
        results: &mut Vec<(Id, String)>,
    ) {
        let mut new_fn_id = current_fn_id;
        let owned_id: Option<Id> = match node.kind() {
            "function_declaration" => node.child_by_field_name("name").and_then(|name_node| {
                let name = &content[name_node.byte_range()];
                let qname = format!("{module_qname}.{name}");
                self.name_to_id.get(&qname).cloned()
            }),
            "variable_declarator" => {
                let is_fn = node
                    .child_by_field_name("value")
                    .map(|v| matches!(v.kind(), "arrow_function" | "function_expression"))
                    .unwrap_or(false);
                if is_fn {
                    node.child_by_field_name("name").and_then(|name_node| {
                        let name = &content[name_node.byte_range()];
                        let qname = format!("{module_qname}.{name}");
                        self.name_to_id.get(&qname).cloned()
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if owned_id.is_some() {
            new_fn_id = owned_id.as_ref();
        }

        if node.kind() == "call_expression" {
            if let Some(from_id) = new_fn_id {
                if let Some(func) = node.child_by_field_name("function") {
                    let callee = match func.kind() {
                        "identifier" => Some(content[func.byte_range()].to_string()),
                        "member_expression" => func
                            .child_by_field_name("property")
                            .map(|p| content[p.byte_range()].to_string()),
                        _ => None,
                    };
                    if let Some(callee_name) = callee {
                        if callee_name != "fetch" && !callee_name.starts_with("axios") {
                            results.push((from_id.clone(), callee_name));
                        }
                    }
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.collect_fn_calls(content, child, module_qname, new_fn_id, results);
            }
        }
    }

    fn resolve_ts_callee(&self, callee: &str, module_qname: &str) -> Option<Id> {
        let qname = format!("{module_qname}.{callee}");
        if let Some(id) = self.name_to_id.get(&qname) {
            return Some(id.clone());
        }
        if let Some(id) = self.name_to_id.get(callee) {
            return Some(id.clone());
        }
        let suffix = format!(".{callee}");
        for (qn, id) in &self.name_to_id {
            if qn.ends_with(&suffix) {
                return Some(id.clone());
            }
        }
        None
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
    fn extract_calls_edges_between_exported_functions() {
        let dir = make_tempdir();
        let code = "export function caller() {\n  return callee();\n}\n\nexport function callee() {\n  return 42;\n}\n";
        let result = extract_ts(&dir, "app.ts", code);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();
        assert!(!calls_edges.is_empty(), "should have Calls edges");
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
}
