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

        ctx.extract_go_files();

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

        // --- Pass 2: walk for call expressions and emit Calls edges ---
        self.extract_calls(root, source, &pkg_qname);

        Ok(())
    }

    fn extract_calls(&mut self, root: tree_sitter::Node, source: &[u8], pkg_qname: &str) {
        let mut fn_calls: Vec<(Id, String)> = Vec::new();
        self.collect_calls_from_node(root, source, pkg_qname, None, &mut fn_calls);

        let mut seen = HashSet::new();
        for (from_id, callee_name) in fn_calls {
            if let Some(to_id) = self.resolve_go_callee(&callee_name, pkg_qname) {
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

    fn collect_calls_from_node(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        pkg_qname: &str,
        current_fn_id: Option<&Id>,
        results: &mut Vec<(Id, String)>,
    ) {
        let mut new_fn_id = current_fn_id;
        let owned_id: Option<Id> = match node.kind() {
            "function_declaration" => {
                let fn_name = find_child_text(&node, "identifier", source);
                if !fn_name.is_empty() {
                    let qname = format!("{pkg_qname}.{fn_name}");
                    self.name_to_id.get(&qname).cloned()
                } else {
                    None
                }
            }
            "method_declaration" => {
                let method_name = find_child_text(&node, "field_identifier", source);
                if !method_name.is_empty() {
                    let receiver_type = extract_receiver_type(&node, source);
                    let qname = if receiver_type.is_empty() {
                        format!("{pkg_qname}.{method_name}")
                    } else {
                        format!("{pkg_qname}.{receiver_type}.{method_name}")
                    };
                    self.name_to_id.get(&qname).cloned()
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
                if let Some(callee) = self.extract_go_call_name(node, source) {
                    results.push((from_id.clone(), callee));
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.collect_calls_from_node(child, source, pkg_qname, new_fn_id, results);
            }
        }
    }

    fn extract_go_call_name(&self, call_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        let func = call_node.child_by_field_name("function")?;
        match func.kind() {
            "identifier" => func.utf8_text(source).ok().map(|s| s.to_string()),
            "selector_expression" => func
                .child_by_field_name("field")
                .and_then(|f| f.utf8_text(source).ok())
                .map(|s| s.to_string()),
            _ => None,
        }
    }

    fn resolve_go_callee(&self, callee: &str, pkg_qname: &str) -> Option<Id> {
        let qname = format!("{pkg_qname}.{callee}");
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
    fn extract_calls_edges() {
        let src = r#"package service

func Caller() {
    Callee()
}

func Callee() int {
    return 42
}
"#;
        let dir = make_repo(GO_MOD, &[("service/svc.go", src)]);
        let result = GoExtractor.extract(dir.path(), "abc123");

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
            "should have Contains edges: package→Handler, package→NewHandler"
        );
    }
}
