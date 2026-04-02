//! Python language extractor — tree-sitter-based parser for the knowledge graph.
//!
//! Walks a Python repository and extracts:
//! - Modules (`.py` files)
//! - Types (`class` definitions)
//! - Functions (`def` at module level, exported if not prefixed with `_`)
//! - Methods (`def` inside a class body)
//! - Endpoints (decorated with Flask/FastAPI route decorators)

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

/// HTTP method names recognized as FastAPI/Flask route decorators.
const ROUTE_METHODS: &[&str] = &[
    "route", "get", "post", "put", "delete", "patch", "options", "head",
];

/// Python language extractor.
///
/// Detects repositories with `pyproject.toml`, `setup.py`, or `requirements.txt`
/// at the root and walks all `.py` files to extract architectural knowledge.
pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn name(&self) -> &str {
        "python"
    }

    fn detect(&self, repo_root: &Path) -> bool {
        repo_root.join("pyproject.toml").is_file()
            || repo_root.join("setup.py").is_file()
            || repo_root.join("requirements.txt").is_file()
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

        // Pass 1: tree-sitter extraction (declarations + Contains edges).
        ctx.extract_python_files();

        // Pass 2: call-graph extraction via external Python script.
        ctx.extract_call_edges();

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
    /// Map qualified name → node Id for edge resolution.
    name_to_id: HashMap<String, Id>,
}

impl ExtractionContext {
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
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
            test_node: false,
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
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }
    }

    // -----------------------------------------------------------------------
    // Python file discovery
    // -----------------------------------------------------------------------

    fn extract_python_files(&mut self) {
        let py_files: Vec<PathBuf> = WalkDir::new(&self.repo_root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                name != "__pycache__" && name != ".git" && name != ".venv" && name != "venv"
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "py").unwrap_or(false))
            .map(|e| e.into_path())
            .collect();

        for path in py_files {
            if let Err(e) = self.extract_py_file(&path) {
                self.errors.push(ExtractionError {
                    file_path: path.display().to_string(),
                    message: e,
                    line: None,
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Single .py file extraction
    // -----------------------------------------------------------------------

    fn extract_py_file(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;
        let source = content.as_bytes();

        let rel_path = path
            .strip_prefix(&self.repo_root)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        let module_qname = path_to_module_qname(&rel_path);
        let module_short = module_qname
            .rsplit('.')
            .next()
            .unwrap_or(&module_qname)
            .to_string();

        // Parse with tree-sitter.
        let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        let tree = parse_source(source, language)?;
        let root = tree.root_node();

        let is_test_file = is_python_test_path(&rel_path);

        // Create Module node.
        let mut module_node = self.make_node(
            NodeType::Module,
            &module_short,
            &module_qname,
            &rel_path,
            1,
            root.end_position().row as u32 + 1,
            Visibility::Public,
        );
        module_node.test_node = is_test_file;
        let module_id = module_node.id.clone();
        self.name_to_id
            .insert(module_qname.clone(), module_id.clone());
        self.nodes.push(module_node);

        // Walk top-level children.
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            match child.kind() {
                "class_definition" => {
                    self.extract_class(child, source, &rel_path, &module_qname, &module_id);
                }
                "function_definition" => {
                    // Module-level function: skip private (leading `_`).
                    if let Some(name) = get_identifier_name(child, source) {
                        if !name.starts_with('_') {
                            self.extract_function_node(
                                child,
                                source,
                                &name,
                                &rel_path,
                                &module_qname,
                                &module_id,
                                false,
                            );
                        }
                    }
                }
                "decorated_definition" => {
                    self.extract_decorated(
                        child,
                        source,
                        &rel_path,
                        &module_qname,
                        &module_id,
                        true,
                    );
                }
                _ => {}
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Class extraction
    // -----------------------------------------------------------------------

    fn extract_class(
        &mut self,
        class_node: tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        module_qname: &str,
        module_id: &Id,
    ) {
        let class_name = match get_identifier_name(class_node, source) {
            Some(n) => n,
            None => return,
        };

        let class_qname = format!("{module_qname}.{class_name}");
        let line_start = class_node.start_position().row as u32 + 1;
        let line_end = class_node.end_position().row as u32 + 1;

        let mut class_node_graph = self.make_node(
            NodeType::Type,
            &class_name,
            &class_qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        // Tag Test* classes and classes in test files/directories.
        if class_name.starts_with("Test") || is_python_test_path(rel_path) {
            class_node_graph.test_node = true;
        }
        let class_id = class_node_graph.id.clone();
        self.name_to_id
            .insert(class_qname.clone(), class_id.clone());
        self.nodes.push(class_node_graph);

        // Contains edge: module → class.
        let edge = self.make_edge(EdgeType::Contains, module_id.clone(), class_id.clone());
        self.edges.push(edge);

        // Track fields we've already seen to avoid duplicates.
        let mut seen_fields: HashSet<String> = HashSet::new();

        // Walk class body for methods and fields.
        if let Some(body) = class_node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                match child.kind() {
                    // Class-level type annotations: `name: str`
                    "expression_statement" => {
                        // Look for assignment or type annotation inside
                        let mut inner_cursor = child.walk();
                        for inner in child.named_children(&mut inner_cursor) {
                            if inner.kind() == "type" {
                                // This is `name: Type` — the parent expression_statement
                                // has a child that is an assignment with type
                            }
                        }
                        // Try to find type annotation: expression_statement > type node
                        // In tree-sitter-python, class-level `x: int` is parsed as
                        // expression_statement > type > identifier (name) + type (annotation)
                        if let Some(type_node) = find_child_by_kind(child, "type") {
                            if let Some(name_text) =
                                type_node.child(0).and_then(|n| n.utf8_text(source).ok())
                            {
                                let type_ann = type_node
                                    .child(2)
                                    .and_then(|n| n.utf8_text(source).ok())
                                    .unwrap_or("?")
                                    .to_string();
                                let field_name = name_text.to_string();
                                if !field_name.starts_with('_')
                                    && seen_fields.insert(field_name.clone())
                                {
                                    self.emit_field_node(
                                        &field_name,
                                        &type_ann,
                                        rel_path,
                                        &class_qname,
                                        &class_id,
                                        child.start_position().row as u32 + 1,
                                    );
                                }
                            }
                        }
                    }
                    "function_definition" => {
                        if let Some(method_name) = get_identifier_name(child, source) {
                            // Extract self.x assignments from __init__
                            if method_name == "__init__" {
                                self.extract_init_fields(
                                    child,
                                    source,
                                    rel_path,
                                    &class_qname,
                                    &class_id,
                                    &mut seen_fields,
                                );
                            }
                            self.extract_function_node(
                                child,
                                source,
                                &method_name,
                                rel_path,
                                &class_qname,
                                &class_id,
                                false,
                            );
                        }
                    }
                    "decorated_definition" => {
                        self.extract_decorated(
                            child,
                            source,
                            rel_path,
                            &class_qname,
                            &class_id,
                            false,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Field node emission
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn emit_field_node(
        &mut self,
        field_name: &str,
        type_ann: &str,
        rel_path: &str,
        class_qname: &str,
        class_id: &Id,
        line: u32,
    ) {
        let field_qname = format!("{class_qname}.{field_name}");
        let mut node = self.make_node(
            NodeType::Field,
            field_name,
            &field_qname,
            rel_path,
            line,
            line,
            Visibility::Public,
        );
        node.doc_comment = Some(type_ann.to_string());
        let field_id = node.id.clone();
        self.name_to_id.insert(field_qname, field_id.clone());
        self.nodes.push(node);

        // FieldOf edge: field → parent class
        let edge = self.make_edge(EdgeType::FieldOf, field_id.clone(), class_id.clone());
        self.edges.push(edge);

        // DependsOn edge if field type refers to a known type
        if let Some(target_id) = self.name_to_id.get(type_ann).cloned() {
            let dep_edge = self.make_edge(EdgeType::DependsOn, field_id, target_id);
            self.edges.push(dep_edge);
        }
    }

    /// Extract `self.x = ...` assignments from `__init__` method body.
    fn extract_init_fields(
        &mut self,
        init_node: tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        class_qname: &str,
        class_id: &Id,
        seen_fields: &mut HashSet<String>,
    ) {
        if let Some(body) = init_node.child_by_field_name("body") {
            self.walk_for_self_assignments(
                body,
                source,
                rel_path,
                class_qname,
                class_id,
                seen_fields,
            );
        }
    }

    fn walk_for_self_assignments(
        &mut self,
        node: tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        class_qname: &str,
        class_id: &Id,
        seen_fields: &mut HashSet<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "expression_statement" => {
                    // Look for assignment: self.x = ...
                    if let Some(assign) = find_child_by_kind(child, "assignment") {
                        if let Some(left) = assign.child_by_field_name("left") {
                            if left.kind() == "attribute" {
                                let obj_text = left
                                    .child_by_field_name("object")
                                    .and_then(|n| n.utf8_text(source).ok());
                                let attr_text = left
                                    .child_by_field_name("attribute")
                                    .and_then(|n| n.utf8_text(source).ok());
                                if obj_text == Some("self") {
                                    if let Some(field_name) = attr_text {
                                        let field_name = field_name.to_string();
                                        if !field_name.starts_with('_')
                                            && seen_fields.insert(field_name.clone())
                                        {
                                            self.emit_field_node(
                                                &field_name,
                                                "?",
                                                rel_path,
                                                class_qname,
                                                class_id,
                                                child.start_position().row as u32 + 1,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Decorated definition extraction
    // -----------------------------------------------------------------------

    fn extract_decorated(
        &mut self,
        decorated_node: tree_sitter::Node,
        source: &[u8],
        rel_path: &str,
        parent_qname: &str,
        parent_id: &Id,
        skip_private: bool,
    ) {
        // Collect all decorator nodes.
        let mut decorators: Vec<tree_sitter::Node> = Vec::new();
        let mut definition: Option<tree_sitter::Node> = None;

        let mut cursor = decorated_node.walk();
        for child in decorated_node.named_children(&mut cursor) {
            match child.kind() {
                "decorator" => decorators.push(child),
                "function_definition" | "class_definition" => definition = Some(child),
                _ => {}
            }
        }

        let def_node = match definition {
            Some(n) => n,
            None => return,
        };

        match def_node.kind() {
            "function_definition" => {
                if let Some(name) = get_identifier_name(def_node, source) {
                    // Skip private module-level functions.
                    if skip_private && name.starts_with('_') {
                        return;
                    }

                    // Check if any decorator is a route decorator → Endpoint.
                    let route_info = decorators
                        .iter()
                        .find_map(|d| parse_route_decorator(*d, source));

                    // Check for Click CLI decorators.
                    let is_click_cmd = decorators.iter().any(|d| is_click_decorator(*d, source));

                    // Check for Celery task decorators.
                    let is_celery_task = decorators.iter().any(|d| is_celery_decorator(*d, source));

                    if let Some((path, method)) = route_info {
                        self.extract_endpoint_node(
                            def_node,
                            source,
                            &name,
                            rel_path,
                            parent_qname,
                            parent_id,
                            &path,
                            &method,
                        );
                    } else if is_click_cmd {
                        self.extract_cli_endpoint_node(
                            def_node,
                            &name,
                            rel_path,
                            parent_qname,
                            parent_id,
                            "click",
                        );
                    } else if is_celery_task {
                        self.extract_cli_endpoint_node(
                            def_node,
                            &name,
                            rel_path,
                            parent_qname,
                            parent_id,
                            "celery",
                        );
                    } else {
                        self.extract_function_node(
                            def_node,
                            source,
                            &name,
                            rel_path,
                            parent_qname,
                            parent_id,
                            false,
                        );
                    }
                }
            }
            "class_definition" => {
                // Decorated class — treat same as plain class.
                let mut cursor2 = decorated_node.walk();
                for child in decorated_node.named_children(&mut cursor2) {
                    if child.kind() == "class_definition" {
                        self.extract_class(child, source, rel_path, parent_qname, parent_id);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Function node creation
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn extract_function_node(
        &mut self,
        fn_node: tree_sitter::Node,
        _source: &[u8],
        name: &str,
        rel_path: &str,
        parent_qname: &str,
        parent_id: &Id,
        _is_module_level: bool,
    ) {
        let qname = format!("{parent_qname}.{name}");
        let line_start = fn_node.start_position().row as u32 + 1;
        let line_end = fn_node.end_position().row as u32 + 1;

        let mut node = self.make_node(
            NodeType::Function,
            name,
            &qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        // Tag test_* functions and functions in test files/directories.
        if name.starts_with("test_") || is_python_test_path(rel_path) {
            node.test_node = true;
        }
        let node_id = node.id.clone();
        self.name_to_id.insert(qname, node_id.clone());
        self.nodes.push(node);

        let edge = self.make_edge(EdgeType::Contains, parent_id.clone(), node_id);
        self.edges.push(edge);
    }

    // -----------------------------------------------------------------------
    // Endpoint node creation
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn extract_endpoint_node(
        &mut self,
        fn_node: tree_sitter::Node,
        _source: &[u8],
        name: &str,
        rel_path: &str,
        parent_qname: &str,
        parent_id: &Id,
        route_path: &str,
        _method: &str,
    ) {
        let qname = format!("{parent_qname}.{name}");
        let line_start = fn_node.start_position().row as u32 + 1;
        let line_end = fn_node.end_position().row as u32 + 1;

        let node = self.make_node(
            NodeType::Endpoint,
            name,
            &qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        let node_id = node.id.clone();
        self.name_to_id.insert(qname, node_id.clone());
        self.nodes.push(node);

        let meta = serde_json::json!({ "path": route_path }).to_string();
        let edge = GraphEdge {
            id: Self::new_id(),
            repo_id: Self::placeholder_repo_id(),
            source_id: parent_id.clone(),
            target_id: node_id,
            edge_type: EdgeType::Contains,
            metadata: Some(meta),
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        };
        self.edges.push(edge);
    }

    // -----------------------------------------------------------------------
    // CLI / task endpoint node creation
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn extract_cli_endpoint_node(
        &mut self,
        fn_node: tree_sitter::Node,
        name: &str,
        rel_path: &str,
        parent_qname: &str,
        parent_id: &Id,
        kind: &str,
    ) {
        let qname = format!("{parent_qname}.{name}");
        let line_start = fn_node.start_position().row as u32 + 1;
        let line_end = fn_node.end_position().row as u32 + 1;

        let mut node = self.make_node(
            NodeType::Endpoint,
            name,
            &qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        node.doc_comment = Some(format!("{kind} entry point"));
        let node_id = node.id.clone();
        self.name_to_id.insert(qname, node_id.clone());
        self.nodes.push(node);

        let meta = serde_json::json!({ "kind": kind }).to_string();
        let edge = GraphEdge {
            id: Self::new_id(),
            repo_id: Self::placeholder_repo_id(),
            source_id: parent_id.clone(),
            target_id: node_id,
            edge_type: EdgeType::Contains,
            metadata: Some(meta),
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        };
        self.edges.push(edge);
    }

    // -----------------------------------------------------------------------
    // Pass 2: Call-graph extraction via external Python script
    // -----------------------------------------------------------------------

    /// Shell out to `scripts/python-callgraph.py` and merge the resulting
    /// `Calls` edges into the graph.  If the script is unavailable or fails,
    /// logs a warning and continues gracefully.
    fn extract_call_edges(&mut self) {
        let script_path = Self::find_callgraph_script();
        let script = match script_path {
            Some(p) => p,
            None => {
                eprintln!("python-callgraph.py not found; skipping call-graph extraction");
                return;
            }
        };

        let output = match Command::new("python3")
            .arg(&script)
            .arg(&self.repo_root)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Failed to run python-callgraph.py: {e}");
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "python-callgraph.py exited with {}: {}",
                output.status,
                stderr.lines().last().unwrap_or("")
            );
            return;
        }

        let stdout = match String::from_utf8(output.stdout) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("python-callgraph.py produced non-UTF-8 output: {e}");
                return;
            }
        };

        let call_edges: Vec<CallEdgeJson> = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse python-callgraph.py JSON: {e}");
                return;
            }
        };

        // Collect existing Calls edges to deduplicate.
        let existing_calls: HashSet<(Id, Id)> = self
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .map(|e| (e.source_id.clone(), e.target_id.clone()))
            .collect();

        let mut added = 0u32;
        for ce in &call_edges {
            let source_id = match self.name_to_id.get(&ce.from) {
                Some(id) => id.clone(),
                None => continue,
            };
            let target_id = match self.name_to_id.get(&ce.to) {
                Some(id) => id.clone(),
                None => continue,
            };
            if source_id == target_id {
                continue;
            }
            if existing_calls.contains(&(source_id.clone(), target_id.clone())) {
                continue;
            }

            let edge = self.make_edge(EdgeType::Calls, source_id, target_id);
            self.edges.push(edge);
            added += 1;
        }

        if added > 0 {
            eprintln!("Pass 2: added {added} Calls edges from python-callgraph.py");
        }
    }

    /// Locate `scripts/python-callgraph.py` relative to the workspace root.
    ///
    /// Searches several candidate locations:
    /// 1. `GYRE_ROOT` environment variable
    /// 2. `CARGO_MANIFEST_DIR` ancestor (works during `cargo test`)
    /// 3. Current executable's ancestor directories
    fn find_callgraph_script() -> Option<PathBuf> {
        let script_name = "scripts/python-callgraph.py";

        // Try GYRE_ROOT env var first.
        if let Ok(root) = std::env::var("GYRE_ROOT") {
            let p = PathBuf::from(root).join(script_name);
            if p.is_file() {
                return Some(p);
            }
        }

        // Try CARGO_MANIFEST_DIR ancestors (works in cargo test).
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let mut dir = PathBuf::from(manifest_dir);
            for _ in 0..5 {
                let p = dir.join(script_name);
                if p.is_file() {
                    return Some(p);
                }
                if !dir.pop() {
                    break;
                }
            }
        }

        // Try current executable's ancestors.
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe;
            for _ in 0..8 {
                if !dir.pop() {
                    break;
                }
                let p = dir.join(script_name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }

        None
    }
}

/// JSON shape emitted by `scripts/python-callgraph.py`.
#[derive(serde::Deserialize)]
struct CallEdgeJson {
    from: String,
    to: String,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a file path indicates a Python test file.
///
/// Matches `test_*.py` filenames and files in `tests/` directories.
fn is_python_test_path(rel_path: &str) -> bool {
    let parts: Vec<&str> = rel_path.split('/').collect();
    // Check for tests/ directory anywhere in the path.
    if parts.iter().any(|&p| p == "tests") {
        return true;
    }
    // Check for test_*.py filename.
    if let Some(filename) = parts.last() {
        if filename.starts_with("test_") && filename.ends_with(".py") {
            return true;
        }
    }
    false
}

/// Derive Python dotted module name from a relative file path.
/// e.g. `src/api/handlers.py` → `src.api.handlers`
fn path_to_module_qname(rel_path: &str) -> String {
    rel_path.trim_end_matches(".py").replace(['/', '\\'], ".")
}

/// Get the `name` field identifier text from a `class_definition` or `function_definition`.
fn get_identifier_name<'a>(node: tree_sitter::Node<'a>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Parse a `decorator` node to determine if it is a route decorator.
///
/// Returns `Some((path, method))` for:
/// - `@app.route("/path")` or `@blueprint.route("/path")`
/// - `@app.get("/path")`, `@router.post("/path")`, etc.
///
/// Returns `None` for any other decorator.
fn parse_route_decorator(decorator: tree_sitter::Node, source: &[u8]) -> Option<(String, String)> {
    // A decorator node's named children (after `@`) can be:
    // - `call` → `@app.route(...)` or `@app.get(...)`
    // - `attribute` → `@app.route` (no parens)
    // - `identifier` → `@route`

    let mut cursor = decorator.walk();
    for child in decorator.named_children(&mut cursor) {
        match child.kind() {
            "call" => {
                // call → function: attribute, arguments
                if let Some(func) = child.child_by_field_name("function") {
                    if let Some((method, path)) = extract_attribute_route(func, child, source) {
                        return Some((path, method));
                    }
                }
            }
            "attribute" => {
                // No-parens form: just check if attribute name is a route method.
                if let Some(attr) = child.child_by_field_name("attribute") {
                    if let Ok(attr_name) = attr.utf8_text(source) {
                        if ROUTE_METHODS.contains(&attr_name) {
                            return Some((String::new(), attr_name.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Given an `attribute` node (`app.route` / `router.get`) and the surrounding
/// `call` node, return `Some((method, path))` if the attribute is a route method.
fn extract_attribute_route(
    func_node: tree_sitter::Node,
    call_node: tree_sitter::Node,
    source: &[u8],
) -> Option<(String, String)> {
    if func_node.kind() != "attribute" {
        return None;
    }

    let attr_node = func_node.child_by_field_name("attribute")?;
    let method = attr_node.utf8_text(source).ok()?;

    if !ROUTE_METHODS.contains(&method) {
        return None;
    }

    // Try to extract the path string from the first positional argument.
    let path = extract_first_string_arg(call_node, source).unwrap_or_default();
    Some((method.to_string(), path))
}

/// Extract the first string literal argument from a `call` node's `argument_list`.
fn extract_first_string_arg(call_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let args = call_node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        match child.kind() {
            "string" => {
                // Strip quotes from the string node text.
                let raw = child.utf8_text(source).ok()?;
                return Some(strip_string_quotes(raw));
            }
            "keyword_argument" => {
                // `path="/foo"` — check if keyword is empty / skip named args before positionals.
                continue;
            }
            _ => {}
        }
    }
    None
}

/// Find the first named child of a given kind.
fn find_child_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

/// Check if a decorator is a Click CLI decorator (`@click.command()`, `@click.group()`).
fn is_click_decorator(decorator: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = decorator.walk();
    for child in decorator.named_children(&mut cursor) {
        match child.kind() {
            "call" => {
                if let Some(func) = child.child_by_field_name("function") {
                    let text = func.utf8_text(source).unwrap_or("");
                    if text == "click.command"
                        || text == "click.group"
                        || text == "cli.command"
                        || text == "cli.group"
                    {
                        return true;
                    }
                    // Also check attribute form: click.command
                    if func.kind() == "attribute" {
                        if let Some(attr) = func.child_by_field_name("attribute") {
                            let attr_name = attr.utf8_text(source).unwrap_or("");
                            if attr_name == "command" || attr_name == "group" {
                                if let Some(obj) = func.child_by_field_name("object") {
                                    let obj_name = obj.utf8_text(source).unwrap_or("");
                                    if obj_name == "click" || obj_name == "cli" {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "attribute" => {
                let text = child.utf8_text(source).unwrap_or("");
                if text == "click.command" || text == "click.group" {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Check if a decorator is a Celery task decorator (`@celery.task`, `@app.task`).
fn is_celery_decorator(decorator: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = decorator.walk();
    for child in decorator.named_children(&mut cursor) {
        match child.kind() {
            "call" => {
                if let Some(func) = child.child_by_field_name("function") {
                    let text = func.utf8_text(source).unwrap_or("");
                    if text.ends_with(".task") {
                        return true;
                    }
                    if func.kind() == "attribute" {
                        if let Some(attr) = func.child_by_field_name("attribute") {
                            let attr_name = attr.utf8_text(source).unwrap_or("");
                            if attr_name == "task" {
                                return true;
                            }
                        }
                    }
                }
            }
            "attribute" => {
                if let Some(attr) = child.child_by_field_name("attribute") {
                    let attr_name = attr.utf8_text(source).unwrap_or("");
                    if attr_name == "task" {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Strip surrounding quotes from a Python string literal.
fn strip_string_quotes(s: &str) -> String {
    let s = s.trim();
    // Handle triple-quoted strings first.
    for q in &[r#"""""#, "'''", r#"""#, r#"'"#] {
        if s.starts_with(q) && s.ends_with(q) && s.len() >= 2 * q.len() {
            return s[q.len()..s.len() - q.len()].to_string();
        }
    }
    s.to_string()
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

    fn make_tempdir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_requirements(dir: &TempDir) {
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
    }

    fn write_pyproject(dir: &TempDir) {
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"my-app\"\n",
        )
        .unwrap();
    }

    #[test]
    fn detect_returns_true_with_pyproject_toml() {
        let dir = make_tempdir();
        write_pyproject(&dir);
        assert!(PythonExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_true_with_requirements_txt() {
        let dir = make_tempdir();
        write_requirements(&dir);
        assert!(PythonExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_true_with_setup_py() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("setup.py"),
            "from setuptools import setup\n",
        )
        .unwrap();
        assert!(PythonExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_false_without_python_markers() {
        let dir = make_tempdir();
        assert!(!PythonExtractor.detect(dir.path()));
    }

    #[test]
    fn extract_class_from_python_file() {
        let dir = make_tempdir();
        write_requirements(&dir);
        fs::write(dir.path().join("models.py"), "class User:\n    pass\n").unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Type && n.name == "User"),
            "should extract User class"
        );
    }

    #[test]
    fn extract_function_from_python_file() {
        let dir = make_tempdir();
        write_requirements(&dir);
        fs::write(
            dir.path().join("utils.py"),
            "def get_config():\n    return {}\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Function && n.name == "get_config"),
            "should extract get_config function"
        );
    }

    #[test]
    fn extract_private_function_not_included() {
        let dir = make_tempdir();
        write_requirements(&dir);
        fs::write(
            dir.path().join("utils.py"),
            "def public_func():\n    pass\n\ndef _private_func():\n    pass\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Function && n.name == "public_func"),
            "should extract public_func"
        );
        assert!(
            !result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Function && n.name == "_private_func"),
            "_private_func should be skipped"
        );
    }

    #[test]
    fn extract_flask_route_as_endpoint() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"from flask import Flask
app = Flask(__name__)

@app.route("/api/users")
def get_users():
    return []
"#;
        fs::write(dir.path().join("app.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Endpoint && n.name == "get_users"),
            "should extract get_users as Endpoint node"
        );
    }

    #[test]
    fn extract_fastapi_route_as_endpoint() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"from fastapi import APIRouter
router = APIRouter()

@router.get("/users")
def list_users():
    return []
"#;
        fs::write(dir.path().join("routes.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Endpoint && n.name == "list_users"),
            "should extract list_users as Endpoint node"
        );
    }

    #[test]
    fn extract_module_node_with_dotted_name() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let src_dir = dir.path().join("src").join("api");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("handlers.py"),
            "class UserHandler:\n    pass\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        let module = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Module && n.qualified_name == "src.api.handlers");
        assert!(module.is_some(), "should have module node src.api.handlers");
    }

    #[test]
    fn contains_edges_connect_module_to_class_and_function() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = "class Repo:\n    pass\n\ndef create():\n    pass\n";
        fs::write(dir.path().join("domain.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        let contains_count = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Contains)
            .count();
        assert!(
            contains_count >= 2,
            "should have at least 2 Contains edges, got {contains_count}"
        );
    }

    #[test]
    fn extract_class_fields_as_field_of_edges() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"class User:
    name: str
    age: int

    def __init__(self, name, age):
        self.name = name
        self.age = age
        self.email = "default"
"#;
        fs::write(dir.path().join("models.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Should have Field nodes.
        let field_nodes: Vec<_> = result
            .nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Field)
            .collect();
        assert!(
            !field_nodes.is_empty(),
            "should extract at least one field node from class"
        );

        // Should have FieldOf edges.
        let field_of_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::FieldOf)
            .collect();
        assert!(
            !field_of_edges.is_empty(),
            "should have FieldOf edges for class fields"
        );
    }

    #[test]
    fn extract_call_edges_cross_module() {
        let dir = make_tempdir();
        write_requirements(&dir);

        // Module A: defines helper()
        fs::write(
            dir.path().join("helpers.py"),
            "def helper():\n    return 42\n",
        )
        .unwrap();

        // Module B: imports helper and calls it
        fs::write(
            dir.path().join("main.py"),
            "from helpers import helper\n\ndef run():\n    helper()\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");

        // Verify both functions exist as nodes.
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.qualified_name == "helpers.helper"),
            "should have helpers.helper node"
        );
        assert!(
            result.nodes.iter().any(|n| n.qualified_name == "main.run"),
            "should have main.run node"
        );

        // Verify a Calls edge was created.
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();

        // The script should resolve main.run -> helpers.helper.
        // If the script is not found this is allowed to be 0 (graceful degradation).
        if ExtractionContext::find_callgraph_script().is_some() {
            assert!(
                !calls_edges.is_empty(),
                "should have at least 1 Calls edge when python-callgraph.py is available"
            );
        }
    }

    #[test]
    fn extract_call_edges_same_module() {
        let dir = make_tempdir();
        write_requirements(&dir);

        // Single module with internal call
        fs::write(
            dir.path().join("service.py"),
            "def validate():\n    return True\n\ndef process():\n    validate()\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");

        if ExtractionContext::find_callgraph_script().is_some() {
            let calls_count = result
                .edges
                .iter()
                .filter(|e| e.edge_type == EdgeType::Calls)
                .count();
            assert!(
                calls_count >= 1,
                "should have Calls edge for same-module call, got {calls_count}"
            );
        }
    }

    #[test]
    fn extract_call_edges_class_method() {
        let dir = make_tempdir();
        write_requirements(&dir);

        // Module with a class calling a module-level function
        let code = r#"def compute():
    return 1

class Handler:
    def handle(self):
        compute()
"#;
        fs::write(dir.path().join("app.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");

        if ExtractionContext::find_callgraph_script().is_some() {
            let calls_count = result
                .edges
                .iter()
                .filter(|e| e.edge_type == EdgeType::Calls)
                .count();
            assert!(
                calls_count >= 1,
                "should have Calls edge from Handler.handle -> compute, got {calls_count}"
            );
        }
    }

    #[test]
    fn test_functions_tagged_as_test_nodes() {
        let dir = make_tempdir();
        write_requirements(&dir);

        // A regular module with a non-test function.
        fs::write(dir.path().join("app.py"), "def serve():\n    pass\n").unwrap();

        // A test file with test_ functions and a Test class.
        let tests_dir = dir.path().join("tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(
            tests_dir.join("test_app.py"),
            "class TestApp:\n    def test_serve(self):\n        pass\n\ndef test_helper():\n    pass\n",
        )
        .unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // test_helper in tests/ directory should be tagged.
        let test_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "test_helper");
        assert!(test_fn.is_some(), "should extract test_helper");
        assert!(
            test_fn.unwrap().test_node,
            "test_helper should be tagged as test_node"
        );

        // TestApp class should be tagged.
        let test_class = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Type && n.name == "TestApp");
        assert!(test_class.is_some(), "should extract TestApp class");
        assert!(
            test_class.unwrap().test_node,
            "TestApp should be tagged as test_node"
        );

        // Regular function should NOT be tagged.
        let prod_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "serve");
        assert!(prod_fn.is_some(), "should extract serve");
        assert!(
            !prod_fn.unwrap().test_node,
            "serve should NOT be tagged as test_node"
        );
    }

    #[test]
    fn extract_click_command_as_endpoint() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"import click

@click.command()
def deploy(env: str):
    """Deploy to environment."""
    pass

@click.group()
def cli():
    pass
"#;
        fs::write(dir.path().join("cli.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let deploy_ep = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "deploy");
        assert!(
            deploy_ep.is_some(),
            "should extract @click.command() 'deploy' as Endpoint node"
        );

        let cli_ep = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "cli");
        assert!(
            cli_ep.is_some(),
            "should extract @click.group() 'cli' as Endpoint node"
        );
    }

    #[test]
    fn extract_celery_task_as_endpoint() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"from celery import Celery

app = Celery('tasks')

@app.task
def process_payment(payment_id: str):
    pass
"#;
        fs::write(dir.path().join("tasks.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let task_ep = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "process_payment");
        assert!(
            task_ep.is_some(),
            "should extract @app.task 'process_payment' as Endpoint node"
        );
    }
}
