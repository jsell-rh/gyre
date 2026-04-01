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
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use walkdir::WalkDir;

/// HTTP method names recognized as FastAPI/Flask route decorators.
const ROUTE_METHODS: &[&str] = &[
    "route", "get", "post", "put", "delete", "patch", "options", "head",
];

/// Common Python base classes that are not domain interfaces — skip for Implements edges.
const PYTHON_SKIP_BASES: &[&str] = &[
    "object",
    "ABC",
    "ABCMeta",
    "BaseModel",
    "Enum",
    "IntEnum",
    "StrEnum",
    "Exception",
    "ValueError",
    "TypeError",
    "RuntimeError",
    "dict",
    "list",
    "tuple",
    "set",
    "str",
    "int",
    "float",
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
        // Check root-level markers first (fast path).
        if repo_root.join("pyproject.toml").is_file()
            || repo_root.join("setup.py").is_file()
            || repo_root.join("requirements.txt").is_file()
        {
            return true;
        }
        // Check one level of subdirectories for monorepos (e.g. src/api/pyproject.toml).
        if let Ok(entries) = std::fs::read_dir(repo_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if p.join("pyproject.toml").is_file()
                        || p.join("setup.py").is_file()
                        || p.join("requirements.txt").is_file()
                    {
                        return true;
                    }
                    // Check one more level (e.g. src/api/)
                    if let Ok(sub_entries) = std::fs::read_dir(&p) {
                        for sub in sub_entries.flatten() {
                            let sp = sub.path();
                            if sp.is_dir()
                                && (sp.join("pyproject.toml").is_file()
                                    || sp.join("setup.py").is_file()
                                    || sp.join("requirements.txt").is_file())
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
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

        ctx.extract_python_files();

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
    /// Map qualified name -> node Id for edge resolution.
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

        // Create Module node.
        let module_node = self.make_node(
            NodeType::Module,
            &module_short,
            &module_qname,
            &rel_path,
            1,
            root.end_position().row as u32 + 1,
            Visibility::Public,
        );
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

        // --- Pass 2: walk for call expressions and emit Calls edges ---
        self.extract_calls(root, source, &module_qname);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Call-site extraction (second pass)
    // -----------------------------------------------------------------------

    fn extract_calls(&mut self, root: tree_sitter::Node, source: &[u8], module_qname: &str) {
        let mut fn_calls: Vec<(Id, String)> = Vec::new();
        self.collect_calls_from_node(root, source, module_qname, None, &mut fn_calls);

        let mut seen = HashSet::new();
        for (from_id, callee_name) in fn_calls {
            if let Some(to_id) = self.resolve_py_callee(&callee_name, module_qname) {
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
        module_qname: &str,
        current_fn_id: Option<&Id>,
        results: &mut Vec<(Id, String)>,
    ) {
        let mut new_fn_id = current_fn_id;
        let owned_id: Option<Id> = if node.kind() == "function_definition" {
            self.resolve_fn_node_id(node, source, module_qname)
        } else {
            None
        };
        if owned_id.is_some() {
            new_fn_id = owned_id.as_ref();
        }

        if node.kind() == "call" {
            if let Some(from_id) = new_fn_id {
                if let Some(callee) = self.extract_call_name(node, source) {
                    results.push((from_id.clone(), callee));
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.collect_calls_from_node(child, source, module_qname, new_fn_id, results);
        }
    }

    fn resolve_fn_node_id(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        module_qname: &str,
    ) -> Option<Id> {
        let name = get_identifier_name(node, source)?;
        let qname = format!("{module_qname}.{name}");
        if let Some(id) = self.name_to_id.get(&qname) {
            return Some(id.clone());
        }
        let parent = node.parent()?;
        if parent.kind() == "block" {
            if let Some(gp) = parent.parent() {
                if gp.kind() == "class_definition" {
                    if let Some(class_name) = get_identifier_name(gp, source) {
                        let method_qname = format!("{module_qname}.{class_name}.{name}");
                        if let Some(id) = self.name_to_id.get(&method_qname) {
                            return Some(id.clone());
                        }
                    }
                }
            }
        }
        None
    }

    fn extract_call_name(&self, call_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        let func = call_node.child_by_field_name("function")?;
        match func.kind() {
            "identifier" => func.utf8_text(source).ok().map(|s| s.to_string()),
            "attribute" => func
                .child_by_field_name("attribute")
                .and_then(|a| a.utf8_text(source).ok())
                .map(|s| s.to_string()),
            _ => None,
        }
    }

    fn resolve_py_callee(&self, callee: &str, module_qname: &str) -> Option<Id> {
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

        let class_node_graph = self.make_node(
            NodeType::Type,
            &class_name,
            &class_qname,
            rel_path,
            line_start,
            line_end,
            Visibility::Public,
        );
        let class_id = class_node_graph.id.clone();
        self.name_to_id
            .insert(class_qname.clone(), class_id.clone());
        self.nodes.push(class_node_graph);

        // Contains edge: module -> class.
        let edge = self.make_edge(EdgeType::Contains, module_id.clone(), class_id.clone());
        self.edges.push(edge);

        // Implements edges from superclass declarations.
        if let Some(superclasses) = class_node.child_by_field_name("superclasses") {
            let mut sc_cursor = superclasses.walk();
            for sc_child in superclasses.named_children(&mut sc_cursor) {
                let base_name = match sc_child.kind() {
                    "identifier" => sc_child.utf8_text(source).unwrap_or("").to_string(),
                    "attribute" => sc_child
                        .child_by_field_name("attribute")
                        .and_then(|a| a.utf8_text(source).ok())
                        .unwrap_or("")
                        .to_string(),
                    _ => continue,
                };
                if base_name.is_empty() || PYTHON_SKIP_BASES.contains(&base_name.as_str()) {
                    continue;
                }
                let target_id =
                    if let Some(id) = self.name_to_id.get(&format!("{module_qname}.{base_name}")) {
                        id.clone()
                    } else if let Some(id) = self.name_to_id.get(&base_name) {
                        id.clone()
                    } else {
                        let suffix = format!(".{base_name}");
                        match self
                            .name_to_id
                            .iter()
                            .find(|(qn, _)| qn.ends_with(&suffix))
                            .map(|(_, id)| id.clone())
                        {
                            Some(id) => id,
                            None => continue,
                        }
                    };
                if class_id != target_id {
                    let impl_edge =
                        self.make_edge(EdgeType::Implements, class_id.clone(), target_id);
                    self.edges.push(impl_edge);
                }
            }
        }

        // Walk class body for methods.
        if let Some(body) = class_node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                match child.kind() {
                    "function_definition" => {
                        if let Some(method_name) = get_identifier_name(child, source) {
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

                    // Check if any decorator is a route decorator -> Endpoint.
                    let route_info = decorators
                        .iter()
                        .find_map(|d| parse_route_decorator(*d, source));

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

        let node = self.make_node(
            NodeType::Function,
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
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Derive Python dotted module name from a relative file path.
/// e.g. `src/api/handlers.py` -> `src.api.handlers`
fn path_to_module_qname(rel_path: &str) -> String {
    rel_path.trim_end_matches(".py").replace(['/', '\\'], ".")
}

/// Get the `name` field identifier text from a `class_definition` or `function_definition`.
fn get_identifier_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Parse a `decorator` node to determine if it is a route decorator.
fn parse_route_decorator(decorator: tree_sitter::Node, source: &[u8]) -> Option<(String, String)> {
    let mut cursor = decorator.walk();
    for child in decorator.named_children(&mut cursor) {
        match child.kind() {
            "call" => {
                if let Some(func) = child.child_by_field_name("function") {
                    if let Some((method, path)) = extract_attribute_route(func, child, source) {
                        return Some((path, method));
                    }
                }
            }
            "attribute" => {
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

    let path = extract_first_string_arg(call_node, source).unwrap_or_default();
    Some((method.to_string(), path))
}

fn extract_first_string_arg(call_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let args = call_node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        match child.kind() {
            "string" => {
                let raw = child.utf8_text(source).ok()?;
                return Some(strip_string_quotes(raw));
            }
            "keyword_argument" => {
                continue;
            }
            _ => {}
        }
    }
    None
}

fn strip_string_quotes(s: &str) -> String {
    let s = s.trim();
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
        let code = "from flask import Flask\napp = Flask(__name__)\n\n@app.route(\"/api/users\")\ndef get_users():\n    return []\n";
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
        let code = "from fastapi import APIRouter\nrouter = APIRouter()\n\n@router.get(\"/users\")\ndef list_users():\n    return []\n";
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
    fn extract_calls_edges() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = "def caller():\n    callee()\n\ndef callee():\n    return 42\n";
        fs::write(dir.path().join("app.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "errors: {:?}",
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
    fn extract_implements_from_superclass() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code =
            "class Repository:\n    def find(self, id):\n        pass\n\nclass SQLRepository(Repository):\n    def find(self, id):\n        return None\n";
        fs::write(dir.path().join("domain.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let impl_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Implements)
            .collect();
        assert!(
            !impl_edges.is_empty(),
            "should have Implements edge from SQLRepository to Repository"
        );
        let sql_id = result
            .nodes
            .iter()
            .find(|n| n.name == "SQLRepository")
            .map(|n| &n.id);
        let repo_id = result
            .nodes
            .iter()
            .find(|n| n.name == "Repository")
            .map(|n| &n.id);
        assert!(sql_id.is_some() && repo_id.is_some());
        assert!(impl_edges
            .iter()
            .any(|e| &e.source_id == sql_id.unwrap() && &e.target_id == repo_id.unwrap()));
    }

    #[test]
    fn skip_common_base_classes() {
        let dir = make_tempdir();
        write_requirements(&dir);
        let code = "from enum import Enum\n\nclass Status(Enum):\n    ACTIVE = 1\n";
        fs::write(dir.path().join("enums.py"), code).unwrap();

        let result = PythonExtractor.extract(dir.path(), "abc123");
        let impl_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Implements)
            .collect();
        assert!(impl_edges.is_empty(), "should NOT emit Implements for Enum");
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
}
