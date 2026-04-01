//! Go language extractor — tree-sitter-based AST parser for the knowledge graph.
//!
//! Walks a Go repository and extracts:
//! - Packages (`package <name>` declarations)
//! - Types (`type Foo struct`)
//! - Interfaces (`type Bar interface`)
//! - Exported functions (`func FunctionName(...)`)
//! - Exported methods (`func (r *Receiver) MethodName(...)`)
//!
//! **qualified_name scheme:** `<go-module-path>/<package>.TypeName`
//! Example: module `github.com/org/myapp`, package `api`, type `Server`
//! → `github.com/org/myapp/api.Server`

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

/// Go language extractor.
///
/// Detects repositories with a `go.mod` at the root and walks all `.go` files
/// to extract architectural knowledge into the graph.
pub struct GoExtractor;

impl LanguageExtractor for GoExtractor {
    fn name(&self) -> &str {
        "go"
    }

    fn detect(&self, repo_root: &Path) -> bool {
        repo_root.join("go.mod").is_file()
    }

    fn extract(&self, repo_root: &Path, commit_sha: &str) -> ExtractionResult {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let module_path = read_module_path(repo_root).unwrap_or_default();

        let mut ctx = GoExtractionContext {
            repo_root: repo_root.to_path_buf(),
            commit_sha: commit_sha.to_string(),
            module_path,
            now,
            nodes: Vec::new(),
            edges: Vec::new(),
            errors: Vec::new(),
            name_to_id: HashMap::new(),
        };

        // Pass 1: tree-sitter AST extraction (declarations + basic edges).
        ctx.extract_go_files();

        // Pass 2: LSP-powered call graph via external Go binary (graceful degradation).
        ctx.extract_lsp_call_graph();

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

struct GoExtractionContext {
    repo_root: PathBuf,
    commit_sha: String,
    /// Module path from go.mod, e.g. `github.com/org/myapp`.
    module_path: String,
    now: u64,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    errors: Vec<ExtractionError>,
    /// qualified_name → node Id for edge resolution.
    name_to_id: HashMap<String, Id>,
}

impl GoExtractionContext {
    fn new_id() -> Id {
        Id::new(Uuid::new_v4().to_string())
    }

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
            first_seen_at: self.now,
            last_seen_at: self.now,
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
            first_seen_at: self.now,
            last_seen_at: self.now,
            deleted_at: None,
        }
    }

    fn extract_go_files(&mut self) {
        let go_files: Vec<PathBuf> = WalkDir::new(&self.repo_root)
            .into_iter()
            .filter_entry(|e| e.file_name() != "vendor")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "go").unwrap_or(false))
            .map(|e| e.into_path())
            .collect();

        for path in go_files {
            if let Err(e) = self.extract_go_file(&path) {
                self.errors.push(ExtractionError {
                    file_path: path.display().to_string(),
                    message: e,
                    line: None,
                });
            }
        }
    }

    fn extract_go_file(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;

        let rel_path = path
            .strip_prefix(&self.repo_root)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        let tree = parse_source(content.as_bytes(), tree_sitter_go::LANGUAGE.into())?;
        let root = tree.root_node();

        // --- Find package name from package_clause ---
        let pkg_name = find_package_name(&root, content.as_bytes());
        if pkg_name.is_empty() {
            // Not a valid Go file with a package declaration; skip.
            return Ok(());
        }

        // Build qualified name for package node: <module>/<pkg>
        let pkg_qname = if self.module_path.is_empty() {
            pkg_name.clone()
        } else {
            format!("{}/{}", self.module_path, pkg_name)
        };

        // Get or create the Package node (shared across files in same package).
        let pkg_id = self.get_or_create_package(&pkg_name, &pkg_qname, &rel_path);

        // --- Walk top-level declarations ---
        let source = content.as_bytes();

        for i in 0..root.child_count() {
            let child = root.child(i).unwrap();
            match child.kind() {
                "type_declaration" => {
                    self.extract_type_decl(
                        &child, source, &rel_path, &pkg_name, &pkg_qname, &pkg_id,
                    );
                }
                "function_declaration" => {
                    self.extract_function_decl(
                        &child, source, &rel_path, &pkg_name, &pkg_qname, &pkg_id,
                    );
                }
                "method_declaration" => {
                    self.extract_method_decl(
                        &child, source, &rel_path, &pkg_name, &pkg_qname, &pkg_id,
                    );
                }
                "import_declaration" => {
                    self.extract_imports(&child, source, &pkg_id);
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Get existing package node or create a new one.
    fn get_or_create_package(&mut self, pkg_name: &str, pkg_qname: &str, file_path: &str) -> Id {
        if let Some(id) = self.name_to_id.get(pkg_qname) {
            return id.clone();
        }
        let node = self.make_node(
            NodeType::Package,
            pkg_name,
            pkg_qname,
            file_path,
            0,
            0,
            Visibility::Public,
        );
        let id = node.id.clone();
        self.name_to_id.insert(pkg_qname.to_string(), id.clone());
        self.nodes.push(node);
        id
    }

    fn extract_type_decl(
        &mut self,
        node: &tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        pkg_name: &str,
        pkg_qname: &str,
        pkg_id: &Id,
    ) {
        // type_declaration may contain one or more type_spec children.
        for i in 0..node.child_count() {
            let child = node.child(i).unwrap();
            if child.kind() != "type_spec" {
                continue;
            }
            // type_spec: type_identifier + (struct_type | interface_type | ...)
            let type_name = find_child_text(&child, "type_identifier", source);
            if type_name.is_empty() {
                continue;
            }

            // Determine if struct or interface.
            let node_type = if has_child_kind(&child, "struct_type") {
                NodeType::Type
            } else if has_child_kind(&child, "interface_type") {
                NodeType::Interface
            } else {
                // Other type aliases — model as Type.
                NodeType::Type
            };

            let line = child.start_position().row as u32 + 1;
            let qname = format!("{pkg_qname}.{type_name}");
            let vis = if is_exported(&type_name) {
                Visibility::Public
            } else {
                Visibility::Private
            };

            let type_node =
                self.make_node(node_type, &type_name, &qname, rel_path, line, line, vis);
            let type_id = type_node.id.clone();
            self.name_to_id.insert(qname, type_id.clone());
            self.nodes.push(type_node);

            // Contains: package → type
            let edge = self.make_edge(EdgeType::Contains, pkg_id.clone(), type_id);
            self.edges.push(edge);

            let _ = pkg_name; // used via pkg_qname
        }
    }

    fn extract_function_decl(
        &mut self,
        node: &tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        _pkg_name: &str,
        pkg_qname: &str,
        pkg_id: &Id,
    ) {
        // function_declaration: identifier + parameter_list + ...
        let fn_name = find_child_text(node, "identifier", source);
        if fn_name.is_empty() || !is_exported(&fn_name) {
            return;
        }

        let line = node.start_position().row as u32 + 1;
        let qname = format!("{pkg_qname}.{fn_name}");

        let fn_node = self.make_node(
            NodeType::Function,
            &fn_name,
            &qname,
            rel_path,
            line,
            line,
            Visibility::Public,
        );
        let fn_id = fn_node.id.clone();
        self.name_to_id.insert(qname, fn_id.clone());
        self.nodes.push(fn_node);

        let edge = self.make_edge(EdgeType::Contains, pkg_id.clone(), fn_id);
        self.edges.push(edge);
    }

    fn extract_method_decl(
        &mut self,
        node: &tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        _pkg_name: &str,
        pkg_qname: &str,
        pkg_id: &Id,
    ) {
        // method_declaration: parameter_list (receiver) + field_identifier (method name)
        let method_name = find_child_text(node, "field_identifier", source);
        if method_name.is_empty() || !is_exported(&method_name) {
            return;
        }

        // Extract receiver type name from the receiver parameter_list.
        let receiver_type = extract_receiver_type(node, source);

        let line = node.start_position().row as u32 + 1;
        let qname = if receiver_type.is_empty() {
            format!("{pkg_qname}.{method_name}")
        } else {
            format!("{pkg_qname}.{receiver_type}.{method_name}")
        };

        let fn_node = self.make_node(
            NodeType::Function,
            &method_name,
            &qname,
            rel_path,
            line,
            line,
            Visibility::Public,
        );
        let fn_id = fn_node.id.clone();
        self.name_to_id.insert(qname, fn_id.clone());
        self.nodes.push(fn_node);

        let edge = self.make_edge(EdgeType::Contains, pkg_id.clone(), fn_id);
        self.edges.push(edge);
    }

    /// Extract import paths and create DependsOn edges for same-module packages.
    fn extract_imports(&mut self, node: &tree_sitter::Node, source: &[u8], pkg_id: &Id) {
        if self.module_path.is_empty() {
            return;
        }
        // import_declaration may have import_spec or import_spec_list children.
        collect_import_paths(node, source, &mut |import_path: &str| {
            if !import_path.starts_with(&self.module_path) {
                return;
            }
            // Same-module import — derive the package qualified name.
            let pkg_suffix = import_path
                .trim_start_matches(&self.module_path)
                .trim_start_matches('/');
            let target_qname = format!("{}/{}", self.module_path, pkg_suffix);

            if let Some(target_id) = self.name_to_id.get(&target_qname).cloned() {
                let edge = self.make_edge(EdgeType::DependsOn, pkg_id.clone(), target_id);
                self.edges.push(edge);
            }
        });
    }

    /// Pass 2: Shell out to `go-callgraph` binary (CHA analysis) and merge
    /// the resulting `Calls` edges into the graph.
    ///
    /// If the binary is not found or fails, logs a warning and continues.
    fn extract_lsp_call_graph(&mut self) {
        let binary = find_go_callgraph_binary();
        let binary = match binary {
            Some(b) => b,
            None => {
                eprintln!(
                    "go_extractor: go-callgraph binary not found; skipping LSP call graph pass"
                );
                return;
            }
        };

        let output = match Command::new(&binary)
            .arg(self.repo_root.to_str().unwrap_or("."))
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("go_extractor: failed to run go-callgraph: {e}");
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "go_extractor: go-callgraph exited with {}: {stderr}",
                output.status
            );
            return;
        }

        let edges: Vec<CallGraphEdge> = match serde_json::from_slice(&output.stdout) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("go_extractor: failed to parse go-callgraph output: {e}");
                return;
            }
        };

        // Build a set of existing Calls edges for deduplication.
        let existing_calls: HashSet<(Id, Id)> = self
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .map(|e| (e.source_id.clone(), e.target_id.clone()))
            .collect();

        for edge in edges {
            let from_id = self.name_to_id.get(&edge.from);
            let to_id = self.name_to_id.get(&edge.to);
            if let (Some(src), Some(tgt)) = (from_id, to_id) {
                let key = (src.clone(), tgt.clone());
                if !existing_calls.contains(&key) {
                    let new_edge = self.make_edge(EdgeType::Calls, src.clone(), tgt.clone());
                    self.edges.push(new_edge);
                }
            }
        }
    }
}

/// A single edge from the go-callgraph JSON output.
#[derive(serde::Deserialize)]
struct CallGraphEdge {
    from: String,
    to: String,
}

/// Locate the `go-callgraph` binary.
///
/// Search order:
/// 1. `GO_CALLGRAPH_BIN` environment variable
/// 2. `scripts/go-callgraph/go-callgraph` relative to the crate manifest dir
/// 3. `go-callgraph` on PATH
fn find_go_callgraph_binary() -> Option<PathBuf> {
    // 1. Explicit env var.
    if let Ok(path) = std::env::var("GO_CALLGRAPH_BIN") {
        let p = PathBuf::from(&path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Relative to workspace root (scripts/go-callgraph/go-callgraph).
    // Walk up from CARGO_MANIFEST_DIR to find the workspace root.
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let mut dir = PathBuf::from(manifest_dir);
        for _ in 0..5 {
            let candidate = dir.join("scripts/go-callgraph/go-callgraph");
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    // 3. On PATH.
    if let Ok(output) = Command::new("which").arg("go-callgraph").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tree-sitter helpers
// ---------------------------------------------------------------------------

/// Find the package name from a `package_clause` node in the file root.
fn find_package_name(root: &tree_sitter::Node, source: &[u8]) -> String {
    for i in 0..root.child_count() {
        let child = root.child(i).unwrap();
        if child.kind() == "package_clause" {
            return find_child_text(&child, "package_identifier", source);
        }
    }
    String::new()
}

/// Get the UTF-8 text of the first child with the given `kind`.
fn find_child_text(node: &tree_sitter::Node, kind: &str, source: &[u8]) -> String {
    for i in 0..node.child_count() {
        let child = node.child(i).unwrap();
        if child.kind() == kind {
            return child.utf8_text(source).unwrap_or("").to_string();
        }
    }
    String::new()
}

/// Return true if any direct child has the given `kind`.
fn has_child_kind(node: &tree_sitter::Node, kind: &str) -> bool {
    for i in 0..node.child_count() {
        if node.child(i).unwrap().kind() == kind {
            return true;
        }
    }
    false
}

/// Extract the receiver type name from a method_declaration node.
///
/// The receiver is the first `parameter_list`. Inside it we look for a
/// `pointer_type` or `type_identifier` to get the receiver's type name.
fn extract_receiver_type(node: &tree_sitter::Node, source: &[u8]) -> String {
    for i in 0..node.child_count() {
        let child = node.child(i).unwrap();
        if child.kind() != "parameter_list" {
            continue;
        }
        // Inside receiver parameter_list: parameter_declaration → type_identifier or pointer_type
        for j in 0..child.child_count() {
            let param = child.child(j).unwrap();
            if param.kind() != "parameter_declaration" {
                continue;
            }
            // Look for type_identifier (value receiver) or pointer_type (pointer receiver).
            for k in 0..param.child_count() {
                let type_node = param.child(k).unwrap();
                match type_node.kind() {
                    "type_identifier" => {
                        return type_node.utf8_text(source).unwrap_or("").to_string();
                    }
                    "pointer_type" => {
                        // pointer_type → "*" + type_identifier
                        let inner = find_child_text(&type_node, "type_identifier", source);
                        if !inner.is_empty() {
                            return inner;
                        }
                    }
                    _ => {}
                }
            }
        }
        break; // Only the first parameter_list is the receiver.
    }
    String::new()
}

/// Walk import_declaration, calling `cb` for each raw import path string.
fn collect_import_paths(node: &tree_sitter::Node, source: &[u8], cb: &mut impl FnMut(&str)) {
    for i in 0..node.child_count() {
        let child = node.child(i).unwrap();
        match child.kind() {
            "import_spec" => {
                // import_spec → interpreted_string_literal
                let raw = find_child_text(&child, "interpreted_string_literal", source);
                let path = raw.trim_matches('"');
                if !path.is_empty() {
                    cb(path);
                }
            }
            "import_spec_list" => {
                collect_import_paths(&child, source, cb);
            }
            _ => {}
        }
    }
}

/// Returns true if the identifier starts with an uppercase letter (Go exported).
fn is_exported(name: &str) -> bool {
    name.chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// go.mod parsing
// ---------------------------------------------------------------------------

/// Read the module path from `go.mod` at the repository root.
///
/// Returns an empty string if the file is missing or unparseable.
fn read_module_path(repo_root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(repo_root.join("go.mod")).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            let module = rest.trim().to_string();
            if !module.is_empty() {
                return Some(module);
            }
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
    use gyre_common::graph::NodeType;
    use std::fs;
    use tempfile::TempDir;

    fn make_repo(go_mod: &str, files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("go.mod"), go_mod).unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        dir
    }

    const GO_MOD: &str = "module github.com/org/myapp\ngo 1.21\n";

    #[test]
    fn detect_returns_true_when_go_mod_exists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), GO_MOD).unwrap();
        assert!(GoExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_false_without_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!GoExtractor.detect(dir.path()));
    }

    #[test]
    fn extract_struct_type_from_go_file() {
        let src = r#"package api

type Server struct {
    addr string
}
"#;
        let dir = make_repo(GO_MOD, &[("api/server.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let type_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Type && n.name == "Server");
        assert!(
            type_node.is_some(),
            "should extract Server struct as Type node"
        );
        let n = type_node.unwrap();
        assert_eq!(n.qualified_name, "github.com/org/myapp/api.Server");
    }

    #[test]
    fn extract_interface_from_go_file() {
        let src = r#"package storage

type Repository interface {
    Find(id string) (interface{}, error)
}
"#;
        let dir = make_repo(GO_MOD, &[("storage/repo.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        let iface_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Interface && n.name == "Repository");
        assert!(
            iface_node.is_some(),
            "should extract Repository as Interface node"
        );
        assert_eq!(
            iface_node.unwrap().qualified_name,
            "github.com/org/myapp/storage.Repository"
        );
    }

    #[test]
    fn extract_exported_function() {
        let src = r#"package service

func CreateUser(name string) error {
    return nil
}
"#;
        let dir = make_repo(GO_MOD, &[("service/user.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        let fn_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "CreateUser");
        assert!(
            fn_node.is_some(),
            "should extract exported CreateUser function"
        );
        assert_eq!(
            fn_node.unwrap().qualified_name,
            "github.com/org/myapp/service.CreateUser"
        );
    }

    #[test]
    fn extract_unexported_function_not_included() {
        let src = r#"package service

func createUser(name string) error {
    return nil
}

func PublicHelper() {}
"#;
        let dir = make_repo(GO_MOD, &[("service/user.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        let unexported = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "createUser");
        assert!(
            unexported.is_none(),
            "unexported functions must NOT be extracted"
        );

        let exported = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "PublicHelper");
        assert!(
            exported.is_some(),
            "exported PublicHelper should be extracted"
        );
    }

    #[test]
    fn extract_exported_method() {
        let src = r#"package api

type Server struct{}

func (s *Server) Start() error {
    return nil
}

func (s *Server) stop() {}
"#;
        let dir = make_repo(GO_MOD, &[("api/server.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        let method_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "Start");
        assert!(
            method_node.is_some(),
            "should extract exported method Start"
        );
        assert_eq!(
            method_node.unwrap().qualified_name,
            "github.com/org/myapp/api.Server.Start"
        );

        let unexported_method = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "stop");
        assert!(
            unexported_method.is_none(),
            "unexported method stop must not be extracted"
        );
    }

    #[test]
    fn contains_edges_link_package_to_types() {
        let src = r#"package api

type Handler struct{}

func NewHandler() *Handler { return nil }
"#;
        let dir = make_repo(GO_MOD, &[("api/handler.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        let contains_count = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Contains)
            .count();
        assert!(
            contains_count >= 2,
            "should have Contains edges: package->Handler, package->NewHandler"
        );
    }

    /// Test Pass 2: LSP call graph extraction with cross-package calls and
    /// interface dispatch.
    ///
    /// This test requires the `go-callgraph` binary to be built first.
    /// It creates a small multi-package Go project and verifies that
    /// cross-package `Calls` edges are produced.
    #[test]
    fn lsp_call_graph_produces_cross_package_calls() {
        // Build the go-callgraph binary if possible.
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        let workspace_root = match workspace_root {
            Some(r) => r,
            None => {
                eprintln!("skipping LSP test: cannot find workspace root");
                return;
            }
        };

        let go_cg_dir = workspace_root.join("scripts/go-callgraph");
        if !go_cg_dir.join("main.go").is_file() {
            eprintln!("skipping LSP test: go-callgraph/main.go not found");
            return;
        }

        // Build the binary into the scripts directory.
        let build_status = std::process::Command::new("go")
            .args(["build", "-o", "go-callgraph", "."])
            .current_dir(&go_cg_dir)
            .status();
        match build_status {
            Ok(s) if s.success() => {}
            _ => {
                eprintln!("skipping LSP test: failed to build go-callgraph");
                return;
            }
        }

        let binary_path = go_cg_dir.join("go-callgraph");
        assert!(binary_path.is_file(), "go-callgraph binary should exist");

        // Create a small multi-package Go project.
        let go_mod = "module example.com/crosscall\ngo 1.21\n";
        let api_src = r#"package api

import "example.com/crosscall/service"

type Handler struct{}

func (h *Handler) Handle() error {
    return service.ProcessRequest("test")
}
"#;
        let service_src = r#"package service

type Processor interface {
    Process(data string) error
}

type DefaultProcessor struct{}

func (d *DefaultProcessor) Process(data string) error {
    return nil
}

func ProcessRequest(data string) error {
    p := &DefaultProcessor{}
    return p.Process(data)
}
"#;
        let dir = make_repo(
            go_mod,
            &[("api/handler.go", api_src), ("service/svc.go", service_src)],
        );

        // Set the env var to point to our built binary.
        std::env::set_var("GO_CALLGRAPH_BIN", binary_path.to_str().unwrap());

        let result = GoExtractor.extract(dir.path(), "abc123");

        // Restore env.
        std::env::remove_var("GO_CALLGRAPH_BIN");

        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Check that cross-package Calls edges exist.
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();

        // We expect at least:
        // - Handler.Handle -> service.ProcessRequest (cross-package call)
        // - ProcessRequest -> DefaultProcessor.Process (intra-package call / interface dispatch)
        assert!(
            calls_edges.len() >= 2,
            "expected at least 2 Calls edges from LSP pass, got {}. \
             All edges: {:?}",
            calls_edges.len(),
            result
                .edges
                .iter()
                .map(|e| format!("{:?}", e.edge_type))
                .collect::<Vec<_>>()
        );

        // Verify specific cross-package edge exists: Handler.Handle -> ProcessRequest.
        let handler_id = result
            .nodes
            .iter()
            .find(|n| n.qualified_name == "example.com/crosscall/api.Handler.Handle")
            .map(|n| &n.id);
        let process_request_id = result
            .nodes
            .iter()
            .find(|n| n.qualified_name == "example.com/crosscall/service.ProcessRequest")
            .map(|n| &n.id);

        if let (Some(from), Some(to)) = (handler_id, process_request_id) {
            let has_edge = calls_edges
                .iter()
                .any(|e| &e.source_id == from && &e.target_id == to);
            assert!(
                has_edge,
                "should have Calls edge from Handler.Handle to ProcessRequest"
            );
        } else {
            panic!(
                "expected both Handler.Handle and ProcessRequest nodes to exist. \
                 Nodes: {:?}",
                result
                    .nodes
                    .iter()
                    .map(|n| &n.qualified_name)
                    .collect::<Vec<_>>()
            );
        }
    }

    /// Test that extraction still works when go-callgraph binary is missing
    /// (graceful degradation).
    #[test]
    fn extraction_degrades_gracefully_without_callgraph_binary() {
        // Set the env var to a nonexistent path.
        std::env::set_var("GO_CALLGRAPH_BIN", "/nonexistent/go-callgraph");

        let src = r#"package api

type Server struct{}

func NewServer() *Server { return nil }
"#;
        let dir = make_repo(GO_MOD, &[("api/server.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

        // Restore env.
        std::env::remove_var("GO_CALLGRAPH_BIN");

        // Pass 1 results should still be present.
        assert!(
            result.errors.is_empty(),
            "should have no errors even without go-callgraph"
        );
        let server_node = result
            .nodes
            .iter()
            .find(|n| n.name == "Server" && n.node_type == NodeType::Type);
        assert!(
            server_node.is_some(),
            "tree-sitter pass should still extract Server type"
        );
    }
}
