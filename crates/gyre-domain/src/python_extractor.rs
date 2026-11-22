//! Python language extractor — tree-sitter-based parser for the knowledge graph.
//!
//! Walks a Python repository and extracts:
//! - Modules (`.py` files)
//! - Types (`class` definitions)
//! - Functions (`def` at module level, exported if not prefixed with `_`)
//! - Methods (`def` inside a class body)
//! - Endpoints (decorated with Flask/FastAPI route decorators, with a
//!   RoutesTo edge to the handler function node)
//! - Implements edges from class inheritance (`class Admin(User)`)
//! - Returns edges from function return annotations (`def f() -> User`)

use crate::edge_resolution::{node_type_index, resolve_deferred_target};
use crate::extractor::{
    shallow_scan_for_marker, ExtractionError, ExtractionResult, LanguageExtractor,
};
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

/// HTTP method names recognized as FastAPI/Flask route decorators.
const ROUTE_METHODS: &[&str] = &[
    "route", "get", "post", "put", "delete", "patch", "options", "head",
];

/// Python language extractor.
///
/// Detects repositories with `pyproject.toml`, `setup.py`, or `requirements.txt`
/// at the root (or in an immediate subdirectory for monorepo layouts) and walks
/// all `.py` files to extract architectural knowledge.
pub struct PythonExtractor;

/// Check for Python project markers at the given path.
fn has_python_markers(dir: &Path) -> bool {
    dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("requirements.txt").is_file()
}

impl LanguageExtractor for PythonExtractor {
    fn name(&self) -> &str {
        "python"
    }

    fn detect(&self, repo_root: &Path) -> bool {
        has_python_markers(repo_root) || shallow_scan_for_marker(repo_root, has_python_markers)
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
            deferred_implements: Vec::new(),
            deferred_returns: Vec::new(),
            deferred_routes: Vec::new(),
        };

        // Pass 1: tree-sitter extraction (declarations + Contains edges).
        ctx.extract_python_files();
        // Deferred-edge resolution: superclasses, return annotations, and
        // route handlers can reference nodes extracted later in the run, so
        // they are resolved only after every file has been extracted.
        ctx.resolve_deferred_implements();
        ctx.resolve_deferred_returns();
        ctx.resolve_deferred_routes();

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
    /// Deferred Implements edges: (class_id, base_class_name) collected from
    /// `class Admin(User)` superclass lists, resolved after all files are
    /// extracted (the base class may live in a later file).
    deferred_implements: Vec<(Id, String)>,
    /// Deferred Returns edges: (function_id, return_type_name) collected from
    /// `-> Type` annotations, resolved after all files are extracted.
    deferred_returns: Vec<(Id, String)>,
    /// Deferred RoutesTo edges: (endpoint_id, handler_qname, metadata) for
    /// Flask/FastAPI route-decorated functions, resolved to the handler
    /// Function node after all files are extracted.
    deferred_routes: Vec<(Id, String, String)>,
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
            spec_paths: vec![],
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
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    fn make_edge(&self, edge_type: EdgeType, source_id: Id, target_id: Id) -> GraphEdge {
        self.make_edge_with_meta(edge_type, source_id, target_id, None)
    }

    fn make_edge_with_meta(
        &self,
        edge_type: EdgeType,
        source_id: Id,
        target_id: Id,
        metadata: Option<String>,
    ) -> GraphEdge {
        GraphEdge {
            id: Self::new_id(),
            repo_id: Self::placeholder_repo_id(),
            source_id,
            target_id,
            edge_type,
            metadata,
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
        // Skip binary files
        if content.contains('\0') {
            return Ok(());
        }
        let source = content.as_bytes();
        // Extract spec governance comments (# spec: <path>)
        let spec_refs = crate::tree_sitter_utils::extract_spec_comments(&content);

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

        // GovernedBy edges from spec comments
        for spec_path in &spec_refs {
            let spec_id = self.name_to_id.get(spec_path).cloned().unwrap_or_else(|| {
                let mut spec_node = self.make_node(
                    NodeType::Module,
                    spec_path,
                    spec_path,
                    spec_path,
                    0,
                    0,
                    Visibility::Public,
                );
                spec_node.spec_path = Some(spec_path.clone());
                spec_node.spec_confidence = SpecConfidence::High;
                let id = spec_node.id.clone();
                self.nodes.push(spec_node);
                self.name_to_id.insert(spec_path.clone(), id.clone());
                id
            });
            let edge = self.make_edge(EdgeType::GovernedBy, module_id.clone(), spec_id);
            self.edges.push(edge);
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

        // Superclass list: `class Admin(User, Base2)` → deferred Implements
        // edges from this class to each resolvable base. Bases that resolve
        // to no known node (stdlib/external imports, generic markers like
        // `Generic`) yield no edge — no fabricated targets. Resolution is
        // deferred because the base class may be extracted later in the run.
        if let Some(supers) = class_node.child_by_field_name("superclasses") {
            let mut cursor = supers.walk();
            for base in supers.named_children(&mut cursor) {
                match base.kind() {
                    // Plain name (`User`) or dotted path (`models.User`).
                    "identifier" | "attribute" => {
                        if let Ok(text) = base.utf8_text(source) {
                            let base_name = text.trim();
                            if !base_name.is_empty() {
                                self.deferred_implements
                                    .push((class_id.clone(), base_name.to_string()));
                            }
                        }
                    }
                    // `Generic[T]` subscripts, `metaclass=...` keyword
                    // arguments, and call expressions resolve to nothing.
                    _ => {}
                }
            }
        }

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
                        // Emit the handler as a Function node too, so the
                        // RoutesTo edge has a real handler target (mirrors
                        // the Go/TypeScript/Rust RoutesTo emitters).
                        self.extract_function_node(
                            def_node,
                            source,
                            &name,
                            rel_path,
                            parent_qname,
                            parent_id,
                            false,
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
        source: &[u8],
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

        let edge = self.make_edge(EdgeType::Contains, parent_id.clone(), node_id.clone());
        self.edges.push(edge);

        // Return annotation: `def f() -> User` → deferred Returns edge from
        // this function to the return-type node. Unresolved names (primitives
        // like int/str, external types) yield no edge. Deferred so the type
        // can be defined in a file extracted later in the run.
        if let Some(ret) = fn_node.child_by_field_name("return_type") {
            if let Some(type_name) = python_return_type_name(ret, source) {
                self.deferred_returns.push((node_id, type_name));
            }
        }
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
        method: &str,
    ) {
        // The endpoint keeps a distinct qualified name from the handler
        // function node (`{parent}.{name}`): both nodes are emitted for a
        // route-decorated function, and graph persistence keys stable-ID
        // remapping on qualified_name — a shared qname would collapse the
        // two nodes onto one ID.
        let qname = format!("{parent_qname}.endpoint.{name}");
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

        let meta = serde_json::json!({ "path": route_path, "method": method }).to_string();
        let edge = self.make_edge_with_meta(
            EdgeType::Contains,
            parent_id.clone(),
            node_id.clone(),
            Some(meta.clone()),
        );
        self.edges.push(edge);

        // RoutesTo edge: endpoint → handler function node. The handler node
        // is emitted by the caller (extract_decorated) right after this;
        // resolution is deferred to after all files so the edge is emitted
        // through the same machinery as the other deferred edges.
        let handler_qname = format!("{parent_qname}.{name}");
        self.deferred_routes.push((node_id, handler_qname, meta));
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
    // Deferred-edge resolution (after all files are extracted)
    // -----------------------------------------------------------------------

    /// Resolve deferred Implements edges: derived class → resolvable base.
    fn resolve_deferred_implements(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_implements);
        let id_to_type = node_type_index(&self.nodes);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for (class_id, base_name) in deferred {
            let Some(target_id) =
                resolve_deferred_target(&base_name, &self.name_to_id, &id_to_type, '.')
            else {
                continue;
            };
            // Skip degenerate self-inheritance and duplicate (class, base) pairs.
            if class_id == target_id {
                continue;
            }
            let key = (class_id.to_string(), target_id.to_string());
            if seen.insert(key) {
                let edge = self.make_edge(EdgeType::Implements, class_id, target_id);
                self.edges.push(edge);
            }
        }
    }

    /// Resolve deferred Returns edges: function → return-type node.
    fn resolve_deferred_returns(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_returns);
        let id_to_type = node_type_index(&self.nodes);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for (fn_id, type_name) in deferred {
            let Some(target_id) =
                resolve_deferred_target(&type_name, &self.name_to_id, &id_to_type, '.')
            else {
                continue;
            };
            let key = (fn_id.to_string(), target_id.to_string());
            if seen.insert(key) {
                let edge = self.make_edge(EdgeType::Returns, fn_id, target_id);
                self.edges.push(edge);
            }
        }
    }

    /// Resolve deferred RoutesTo edges: route endpoint → handler function.
    fn resolve_deferred_routes(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_routes);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for (endpoint_id, handler_qname, meta) in deferred {
            let Some(handler_id) = self.name_to_id.get(&handler_qname).cloned() else {
                continue;
            };
            let key = (endpoint_id.to_string(), handler_id.to_string());
            if seen.insert(key) {
                let edge = self.make_edge_with_meta(
                    EdgeType::RoutesTo,
                    endpoint_id,
                    handler_id,
                    Some(meta),
                );
                self.edges.push(edge);
            }
        }
    }
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

/// Normalize a Python return-annotation `type` node into a resolvable type name.
///
/// Handles the common annotation forms:
/// - `-> User` → `User`
/// - `-> models.User` → `models.User` (suffix resolution finds `src.models.User`)
/// - `-> "User"` (postponed string annotation) → `User`
/// - `-> Optional[User]` / `-> list[User]` → the outer name (`Optional`/`list`),
///   which resolves to no repo node and therefore yields no edge — the same
///   generics-stripping rule the Rust extractor applies to `Vec<T>`.
///
/// Returns `None` for empty/unknown annotations.
fn python_return_type_name(ret_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let text = ret_node.utf8_text(source).ok()?.trim();
    if text.is_empty() || text == "?" {
        return None;
    }
    // Postponed annotations are string literals: strip the quotes.
    let unquoted = strip_string_quotes(text);
    // Strip generic parameters (`Optional[User]` → `Optional`).
    let base = unquoted.split('[').next().unwrap_or(&unquoted).trim();
    if base.is_empty() || base == "?" {
        None
    } else {
        Some(base.to_string())
    }
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

    // -----------------------------------------------------------------------
    // Implements edges (class inheritance)
    // -----------------------------------------------------------------------

    fn node_id_by_name(result: &ExtractionResult, name: &str) -> Option<Id> {
        result
            .nodes
            .iter()
            .find(|n| n.name == name)
            .map(|n| n.id.clone())
    }

    fn has_edge(result: &ExtractionResult, et: EdgeType, from_name: &str, to_name: &str) -> bool {
        match (
            node_id_by_name(result, from_name),
            node_id_by_name(result, to_name),
        ) {
            (Some(f), Some(t)) => result
                .edges
                .iter()
                .any(|e| e.edge_type == et && e.source_id == f && e.target_id == t),
            _ => false,
        }
    }

    #[test]
    fn class_inheritance_emits_implements_edge_with_base_defined_after() {
        let dir = make_tempdir();
        write_requirements(&dir);
        // User is declared AFTER Admin in the same file: at the moment Admin
        // is extracted, no User node exists yet. The deferred resolution must
        // still produce the Implements edge (order-independence).
        let code = "class Admin(User, Base2):\n    pass\n\nclass User:\n    pass\n\nclass Base2:\n    pass\n";
        fs::write(dir.path().join("models.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        assert!(
            has_edge(&result, EdgeType::Implements, "Admin", "User"),
            "class Admin(User) must emit an Implements edge Admin -> User"
        );
        // Multiple inheritance: one edge per resolvable base.
        assert!(
            has_edge(&result, EdgeType::Implements, "Admin", "Base2"),
            "class Admin(User, Base2) must emit an Implements edge Admin -> Base2"
        );
    }

    #[test]
    fn class_inheritance_across_files_in_both_extraction_orders() {
        // Base class in a separate file, derived class in another. The edge
        // must exist no matter which file the walk visits first.
        for (admin_first, label) in [(true, "admin file first"), (false, "user file first")] {
            let dir = make_tempdir();
            write_requirements(&dir);
            let files: Vec<(&str, &str)> = if admin_first {
                vec![
                    (
                        "admin.py",
                        "from models import User\n\nclass Admin(User):\n    pass\n",
                    ),
                    ("models.py", "class User:\n    pass\n"),
                ]
            } else {
                vec![
                    ("models.py", "class User:\n    pass\n"),
                    (
                        "admin.py",
                        "from models import User\n\nclass Admin(User):\n    pass\n",
                    ),
                ]
            };
            for (name, content) in files {
                fs::write(dir.path().join(name), content).unwrap();
            }

            let result = PythonExtractor.extract(dir.path(), "abc123");
            assert!(
                result.errors.is_empty(),
                "unexpected errors ({}): {:?}",
                label,
                result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
            assert!(
                has_edge(&result, EdgeType::Implements, "Admin", "User"),
                "cross-file class Admin(User) must emit Implements edge ({label})"
            );
        }
    }

    #[test]
    fn unresolvable_base_yields_no_implements_edge() {
        let dir = make_tempdir();
        write_requirements(&dir);
        // No node named ExternalBase or Generic exists in the repo — no
        // fabricated targets.
        let code = "from typing import Generic\n\nclass Foo(ExternalBase, Generic[T]):\n    pass\n";
        fs::write(dir.path().join("models.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let implements_count = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Implements)
            .count();
        assert_eq!(
            implements_count, 0,
            "unresolvable bases must yield no Implements edges"
        );
    }

    // -----------------------------------------------------------------------
    // Returns edges (function signatures)
    // -----------------------------------------------------------------------

    #[test]
    fn return_annotation_emits_returns_edge() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = "class User:\n    pass\n\ndef build_user() -> User:\n    return User()\n\ndef count_items() -> int:\n    return 0\n";
        fs::write(dir.path().join("models.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        assert!(
            has_edge(&result, EdgeType::Returns, "build_user", "User"),
            "def build_user() -> User must emit a Returns edge build_user -> User"
        );
        assert!(
            !has_edge(&result, EdgeType::Returns, "count_items", "int"),
            "primitive return type must yield no Returns edge"
        );
        let returns_count = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Returns)
            .count();
        assert_eq!(returns_count, 1, "exactly one Returns edge expected");
    }

    // -----------------------------------------------------------------------
    // RoutesTo edges (framework patterns)
    // -----------------------------------------------------------------------

    #[test]
    fn flask_route_emits_endpoint_handler_and_routes_to_edge() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = r#"from flask import Flask
app = Flask(__name__)

@app.route("/api/users")
def list_users():
    return []
"#;
        fs::write(dir.path().join("app.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let endpoint = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "list_users")
            .expect("route-decorated function must yield an Endpoint node");
        let handler = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "list_users")
            .expect("route handler must also be emitted as a Function node");
        assert_ne!(
            endpoint.id, handler.id,
            "endpoint and handler must be distinct nodes"
        );
        assert_ne!(
            endpoint.qualified_name, handler.qualified_name,
            "endpoint and handler must not share a qualified name (stable-ID remapping is qname-keyed)"
        );

        let routes_edge = result
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::RoutesTo && e.source_id == endpoint.id)
            .expect("must emit a RoutesTo edge from the endpoint");
        assert_eq!(
            routes_edge.target_id, handler.id,
            "RoutesTo edge must target the handler Function node"
        );
        let meta: serde_json::Value =
            serde_json::from_str(routes_edge.metadata.as_deref().unwrap_or("{}"))
                .expect("RoutesTo metadata must be valid JSON");
        assert_eq!(
            meta["path"], "/api/users",
            "metadata must carry the route path"
        );
        assert_eq!(
            meta["method"], "route",
            "metadata must carry the HTTP method"
        );
    }
}
