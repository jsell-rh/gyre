//! Rust language extractor — syn-based AST parser for the knowledge graph.
//!
//! Walks a Rust repository and extracts:
//! - Packages (Cargo.toml)
//! - Modules (`.rs` files)
//! - Types (`struct`, `enum`)
//! - Interfaces (`trait`)
//! - Implementations (`impl Trait for Type` → Implements edge)
//! - Functions (`pub fn`)
//! - Endpoints (axum `.route(...)` macros)
//! - Tables (`diesel::table!` macros)
//! - Spec governance (`// spec: <path>` comments)

use crate::extractor::{ExtractionError, ExtractionResult, LanguageExtractor};
use gyre_common::{
    graph::{EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence, Visibility},
    Id,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use syn::{visit::Visit, Item};
use uuid::Uuid;
use walkdir::WalkDir;

/// Rust language extractor.
///
/// Detects repositories with a `Cargo.toml` at the root and walks all `.rs`
/// files to extract architectural knowledge into the graph.
pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn name(&self) -> &str {
        "rust"
    }

    fn detect(&self, repo_root: &Path) -> bool {
        repo_root.join("Cargo.toml").is_file()
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

        ctx.extract_packages();
        ctx.extract_rust_files();

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
        doc_comment: Option<String>,
        spec_path: Option<String>,
        spec_confidence: SpecConfidence,
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
            doc_comment,
            spec_path,
            spec_confidence,
            last_modified_sha: self.commit_sha.clone(),
            last_modified_by: None,
            last_modified_at: self.now,
            created_sha: self.commit_sha.clone(),
            created_at: self.now,
            complexity: None,
            churn_count_30d: 0,
            test_coverage: None,
            // Time-travel fields are initialised to 0 / None here;
            // graph_extraction.rs fills in real timestamps after extraction.
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
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
    // Package extraction (Cargo.toml)
    // -----------------------------------------------------------------------

    fn extract_packages(&mut self) {
        let manifests: Vec<PathBuf> = WalkDir::new(&self.repo_root)
            .into_iter()
            .filter_entry(|e| e.file_name() != "target")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "Cargo.toml")
            .map(|e| e.into_path())
            .collect();

        let mut package_ids: HashMap<String, Id> = HashMap::new();

        // First pass: create Package nodes.
        for manifest_path in &manifests {
            match self.parse_cargo_toml(manifest_path) {
                Ok(Some((name, _version, _deps))) => {
                    let rel_path = manifest_path
                        .strip_prefix(&self.repo_root)
                        .ok()
                        .and_then(|p| p.to_str())
                        .unwrap_or("")
                        .to_string();

                    let node = self.make_node(
                        NodeType::Package,
                        &name,
                        &name,
                        &rel_path,
                        0,
                        0,
                        Visibility::Public,
                        None,
                        None,
                        SpecConfidence::None,
                    );
                    let node_id = node.id.clone();
                    self.name_to_id.insert(name.clone(), node_id.clone());
                    package_ids.insert(name, node_id);
                    self.nodes.push(node);
                }
                Ok(None) => {} // workspace root without [package]
                Err(e) => self.errors.push(ExtractionError {
                    file_path: manifest_path.display().to_string(),
                    message: e,
                    line: None,
                }),
            }
        }

        // Second pass: create DependsOn edges from declared [dependencies].
        for manifest_path in &manifests {
            if let Ok(Some((pkg_name, _version, deps))) = self.parse_cargo_toml(manifest_path) {
                if let Some(from_id) = package_ids.get(&pkg_name).cloned() {
                    for dep_name in &deps {
                        if let Some(to_id) = package_ids.get(dep_name).cloned() {
                            let edge = self.make_edge(EdgeType::DependsOn, from_id.clone(), to_id);
                            self.edges.push(edge);
                        }
                    }
                }
            }
        }
    }

    fn parse_cargo_toml(
        &self,
        path: &Path,
    ) -> Result<Option<(String, String, Vec<String>)>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;

        let table: toml::Table = content
            .parse()
            .map_err(|e| format!("TOML parse error: {e}"))?;

        let package = match table.get("package").and_then(|v| v.as_table()) {
            Some(p) => p,
            None => return Ok(None),
        };

        let name = package
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0")
            .to_string();

        let mut deps: Vec<String> = Vec::new();
        if let Some(dep_table) = table.get("dependencies").and_then(|v| v.as_table()) {
            for dep_name in dep_table.keys() {
                deps.push(dep_name.clone());
            }
        }

        Ok(Some((name, version, deps)))
    }

    // -----------------------------------------------------------------------
    // Rust file extraction
    // -----------------------------------------------------------------------

    fn extract_rust_files(&mut self) {
        let rs_files: Vec<PathBuf> = WalkDir::new(&self.repo_root)
            .into_iter()
            .filter_entry(|e| e.file_name() != "target")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "rs").unwrap_or(false))
            .map(|e| e.into_path())
            .collect();

        for path in rs_files {
            if let Err(e) = self.extract_rs_file(&path) {
                self.errors.push(ExtractionError {
                    file_path: path.display().to_string(),
                    message: e,
                    line: None,
                });
            }
        }
    }

    fn extract_rs_file(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;

        let rel_path = path
            .strip_prefix(&self.repo_root)
            .ok()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        let (crate_name, module_prefix) = crate_and_module_from_path(&rel_path);

        let module_qname = if module_prefix.is_empty() {
            crate_name.clone()
        } else {
            format!("{crate_name}::{module_prefix}")
        };

        // Collect spec governance comments.
        let spec_refs = extract_spec_comments(&content);
        let primary_spec = spec_refs.first().cloned();
        let spec_confidence = if primary_spec.is_some() {
            SpecConfidence::High
        } else {
            SpecConfidence::None
        };

        let module_short_name = module_prefix
            .rsplit("::")
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&crate_name)
            .to_string();

        let module_node = self.make_node(
            NodeType::Module,
            &module_short_name,
            &module_qname,
            &rel_path,
            1,
            0,
            Visibility::Public,
            None,
            primary_spec,
            spec_confidence,
        );
        let module_id = module_node.id.clone();
        self.name_to_id
            .insert(module_qname.clone(), module_id.clone());
        self.nodes.push(module_node);

        let syntax = syn::parse_file(&content).map_err(|e| format!("syn parse error: {e}"))?;

        // GovernedBy edges: module → spec node.
        for spec_ref in &spec_refs {
            let spec_id = self.get_or_create_spec_node(spec_ref);
            let edge = self.make_edge_with_meta(
                EdgeType::GovernedBy,
                module_id.clone(),
                spec_id,
                Some(r#"{"confidence":"high"}"#.to_string()),
            );
            self.edges.push(edge);
        }

        let mut visitor = ItemVisitor {
            ctx: self,
            rel_path: &rel_path,
            crate_name: &crate_name,
            module_qname: &module_qname,
            module_id: &module_id,
            new_nodes: Vec::new(),
            new_edges: Vec::new(),
        };
        visitor.visit_file(&syntax);

        let new_nodes = std::mem::take(&mut visitor.new_nodes);
        let new_edges = std::mem::take(&mut visitor.new_edges);
        drop(visitor);

        self.nodes.extend(new_nodes);
        self.edges.extend(new_edges);

        self.extract_endpoints_from_text(&content, &rel_path, &module_id);
        self.extract_diesel_tables(&content, &rel_path, &module_id);

        // --- Pass 2: walk function bodies for Calls edges ---
        let mut call_visitor = CallVisitor {
            name_to_id: &self.name_to_id,
            module_qname: module_qname.clone(),
            current_fn_id: None,
            new_edges: Vec::new(),
            seen: HashSet::new(),
        };
        call_visitor.visit_file(&syntax);
        self.edges.extend(call_visitor.new_edges);

        Ok(())
    }

    /// Get or create a Module node representing a spec file (GovernedBy edge target).
    fn get_or_create_spec_node(&mut self, spec_path: &str) -> Id {
        if let Some(id) = self.name_to_id.get(spec_path) {
            return id.clone();
        }
        let node = self.make_node(
            NodeType::Module,
            spec_path,
            spec_path,
            spec_path,
            0,
            0,
            Visibility::Public,
            None,
            None,
            SpecConfidence::None,
        );
        let id = node.id.clone();
        self.name_to_id.insert(spec_path.to_string(), id.clone());
        self.nodes.push(node);
        id
    }

    // -----------------------------------------------------------------------
    // Axum endpoint extraction (text-based pattern matching)
    // -----------------------------------------------------------------------

    fn extract_endpoints_from_text(&mut self, content: &str, rel_path: &str, module_id: &Id) {
        for (line_idx, text) in content.lines().enumerate() {
            let line_num = (line_idx + 1) as u32;
            let text_trimmed = text.trim();

            if !text_trimmed.contains(".route(") {
                continue;
            }

            if let Some((route_path, method, handler)) = parse_route_line(text_trimmed) {
                let endpoint_name = route_path
                    .replace('/', "_")
                    .trim_start_matches('_')
                    .to_string();
                let endpoint_qname =
                    format!("{}::{}", rel_path.trim_end_matches(".rs"), &endpoint_name);
                let meta = serde_json::json!({ "path": route_path, "method": method }).to_string();

                let endpoint_node = self.make_node(
                    NodeType::Endpoint,
                    &endpoint_name,
                    &endpoint_qname,
                    rel_path,
                    line_num,
                    line_num,
                    Visibility::Public,
                    None,
                    None,
                    SpecConfidence::None,
                );
                let endpoint_id = endpoint_node.id.clone();
                self.nodes.push(endpoint_node);

                if let Some(handler_id) = self.name_to_id.get(&handler).cloned() {
                    let edge = self.make_edge_with_meta(
                        EdgeType::RoutesTo,
                        endpoint_id.clone(),
                        handler_id,
                        Some(meta.clone()),
                    );
                    self.edges.push(edge);
                }

                let edge = self.make_edge_with_meta(
                    EdgeType::Contains,
                    module_id.clone(),
                    endpoint_id,
                    Some(meta),
                );
                self.edges.push(edge);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Diesel table! macro extraction
    // -----------------------------------------------------------------------

    fn extract_diesel_tables(&mut self, content: &str, rel_path: &str, module_id: &Id) {
        if !rel_path.ends_with("schema.rs")
            && !content.contains("diesel::table!")
            && !content.contains("table! {")
        {
            return;
        }

        let mut in_table_block = false;
        let mut brace_depth = 0u32;
        let mut table_name = String::new();
        let mut columns: Vec<String> = Vec::new();
        let mut table_start_line = 0u32;

        for (line_idx, line) in content.lines().enumerate() {
            let line_num = (line_idx + 1) as u32;
            let trimmed = line.trim();

            if !in_table_block {
                if (trimmed.starts_with("table!") || trimmed.contains("diesel::table!"))
                    && trimmed.contains('{')
                {
                    in_table_block = true;
                    brace_depth = trimmed.chars().filter(|&c| c == '{').count() as u32
                        - trimmed.chars().filter(|&c| c == '}').count() as u32;
                    table_start_line = line_num;
                    if let Some(name) = extract_table_name_from_line(trimmed) {
                        table_name = name;
                    }
                    columns.clear();
                }
            } else {
                let open = trimmed.chars().filter(|&c| c == '{').count() as u32;
                let close = trimmed.chars().filter(|&c| c == '}').count() as u32;

                if table_name.is_empty() {
                    if let Some(name) = extract_table_name_from_line(trimmed) {
                        table_name = name;
                    }
                }

                if trimmed.contains("->") && !trimmed.starts_with("//") {
                    if let Some(col) = trimmed.split("->").next() {
                        let col_name = col.trim().to_string();
                        if !col_name.is_empty() {
                            columns.push(col_name);
                        }
                    }
                }

                brace_depth = brace_depth.saturating_add(open).saturating_sub(close);

                if brace_depth == 0 {
                    if !table_name.is_empty() {
                        let meta = serde_json::json!({ "columns": columns }).to_string();
                        let node = self.make_node(
                            NodeType::Table,
                            &table_name,
                            &table_name,
                            rel_path,
                            table_start_line,
                            line_num,
                            Visibility::Public,
                            None,
                            None,
                            SpecConfidence::None,
                        );
                        let table_id = node.id.clone();
                        self.name_to_id.insert(table_name.clone(), table_id.clone());
                        self.nodes.push(node);

                        let edge = self.make_edge_with_meta(
                            EdgeType::Contains,
                            module_id.clone(),
                            table_id,
                            Some(meta),
                        );
                        self.edges.push(edge);
                    }

                    in_table_block = false;
                    table_name.clear();
                    columns.clear();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// syn AST visitor
// ---------------------------------------------------------------------------

struct ItemVisitor<'a> {
    ctx: &'a mut ExtractionContext,
    rel_path: &'a str,
    crate_name: &'a str,
    module_qname: &'a str,
    module_id: &'a Id,
    new_nodes: Vec<GraphNode>,
    new_edges: Vec<GraphEdge>,
}

impl<'a> ItemVisitor<'a> {
    fn qualified(&self, name: &str) -> String {
        if self.module_qname == self.crate_name {
            format!("{}::{}", self.crate_name, name)
        } else {
            format!("{}::{}", self.module_qname, name)
        }
    }

    fn make_node(
        &self,
        node_type: NodeType,
        name: &str,
        line: u32,
        vis: Visibility,
        doc: Option<String>,
    ) -> GraphNode {
        let qname = self.qualified(name);
        self.ctx.make_node(
            node_type,
            name,
            &qname,
            self.rel_path,
            line,
            line,
            vis,
            doc,
            None,
            SpecConfidence::None,
        )
    }

    fn add_contains_edge(&self, child_id: Id) -> GraphEdge {
        self.ctx
            .make_edge(EdgeType::Contains, self.module_id.clone(), child_id)
    }
}

impl<'ast, 'a> Visit<'ast> for ItemVisitor<'a> {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Struct(s) => {
                let name = s.ident.to_string();
                let vis = syn_vis_to_visibility(&s.vis);
                let doc = extract_doc_comment(&s.attrs);
                let node = self.make_node(NodeType::Type, &name, 0, vis, doc);
                let node_id = node.id.clone();
                self.ctx
                    .name_to_id
                    .insert(self.qualified(&name), node_id.clone());
                let edge = self.add_contains_edge(node_id);
                self.new_nodes.push(node);
                self.new_edges.push(edge);
            }
            Item::Enum(e) => {
                let name = e.ident.to_string();
                let vis = syn_vis_to_visibility(&e.vis);
                let doc = extract_doc_comment(&e.attrs);
                let node = self.make_node(NodeType::Type, &name, 0, vis, doc);
                let node_id = node.id.clone();
                self.ctx
                    .name_to_id
                    .insert(self.qualified(&name), node_id.clone());
                let edge = self.add_contains_edge(node_id);
                self.new_nodes.push(node);
                self.new_edges.push(edge);
            }
            Item::Trait(t) => {
                let name = t.ident.to_string();
                let vis = syn_vis_to_visibility(&t.vis);
                let doc = extract_doc_comment(&t.attrs);
                let node = self.make_node(NodeType::Interface, &name, 0, vis, doc);
                let node_id = node.id.clone();
                self.ctx
                    .name_to_id
                    .insert(self.qualified(&name), node_id.clone());
                let edge = self.add_contains_edge(node_id);
                self.new_nodes.push(node);
                self.new_edges.push(edge);
            }
            Item::Impl(imp) => {
                if let (Some(trait_path), syn::Type::Path(type_path)) = (&imp.trait_, &*imp.self_ty)
                {
                    let trait_name = path_to_string(&trait_path.1);
                    let type_name = path_to_string(&type_path.path);

                    let trait_qname = self.qualified(&trait_name);
                    let type_qname = self.qualified(&type_name);

                    let from_id = self.ctx.name_to_id.get(&type_qname).cloned();
                    let to_id = self.ctx.name_to_id.get(&trait_qname).cloned();

                    if let (Some(from_id), Some(to_id)) = (from_id, to_id) {
                        let edge = self.ctx.make_edge(EdgeType::Implements, from_id, to_id);
                        self.new_edges.push(edge);
                    }
                }
            }
            Item::Fn(f) => {
                if matches!(f.vis, syn::Visibility::Public(_)) {
                    let name = f.sig.ident.to_string();
                    let doc = extract_doc_comment(&f.attrs);
                    let sig = format_fn_sig(&f.sig);
                    let mut node =
                        self.make_node(NodeType::Function, &name, 0, Visibility::Public, doc);
                    if node.doc_comment.is_none() {
                        node.doc_comment = Some(sig);
                    }
                    let node_id = node.id.clone();
                    self.ctx
                        .name_to_id
                        .insert(self.qualified(&name), node_id.clone());
                    let edge = self.add_contains_edge(node_id);
                    self.new_nodes.push(node);
                    self.new_edges.push(edge);
                }
            }
            Item::Mod(m) => {
                syn::visit::visit_item_mod(self, m);
            }
            _ => {
                syn::visit::visit_item(self, item);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Call-site visitor (second pass — emits Calls edges)
// ---------------------------------------------------------------------------

struct CallVisitor<'a> {
    name_to_id: &'a HashMap<String, Id>,
    module_qname: String,
    current_fn_id: Option<Id>,
    new_edges: Vec<GraphEdge>,
    seen: HashSet<(String, String)>,
}

impl<'a> CallVisitor<'a> {
    fn emit_call(&mut self, from_id: &Id, to_id: &Id) {
        let key = (from_id.to_string(), to_id.to_string());
        if self.seen.contains(&key) {
            return;
        }
        self.seen.insert(key);
        self.new_edges.push(GraphEdge {
            id: ExtractionContext::new_id(),
            repo_id: ExtractionContext::placeholder_repo_id(),
            source_id: from_id.clone(),
            target_id: to_id.clone(),
            edge_type: EdgeType::Calls,
            metadata: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        });
    }

    fn resolve_callee(&self, callee: &str) -> Option<Id> {
        let qname = format!("{}::{}", self.module_qname, callee);
        if let Some(id) = self.name_to_id.get(&qname) {
            return Some(id.clone());
        }
        if let Some(id) = self.name_to_id.get(callee) {
            return Some(id.clone());
        }
        let short = callee.rsplit("::").next().unwrap_or(callee);
        if short != callee {
            let qname2 = format!("{}::{}", self.module_qname, short);
            if let Some(id) = self.name_to_id.get(&qname2) {
                return Some(id.clone());
            }
        }
        None
    }
}

impl<'ast, 'a> Visit<'ast> for CallVisitor<'a> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let name = f.sig.ident.to_string();
        let qname = format!("{}::{}", self.module_qname, name);
        let prev = self.current_fn_id.take();
        self.current_fn_id = self.name_to_id.get(&qname).cloned();
        syn::visit::visit_item_fn(self, f);
        self.current_fn_id = prev;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some(from_id) = self.current_fn_id.clone() {
            if let syn::Expr::Path(ep) = &*call.func {
                let callee = path_to_string(&ep.path);
                if let Some(to_id) = self.resolve_callee(&callee) {
                    if from_id != to_id {
                        self.emit_call(&from_id, &to_id);
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        if let Some(from_id) = self.current_fn_id.clone() {
            let method_name = mc.method.to_string();
            let suffix = format!("::{}", method_name);
            let target = self
                .name_to_id
                .iter()
                .find(|(qname, id)| qname.ends_with(&suffix) && from_id != **id)
                .map(|(_, id)| id.clone());
            if let Some(to_id) = target {
                self.emit_call(&from_id, &to_id);
            }
        }
        syn::visit::visit_expr_method_call(self, mc);
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Derive (crate_name, module_prefix) from a relative file path.
/// e.g. `crates/gyre-domain/src/dependency.rs` -> (`gyre-domain`, `dependency`)
fn crate_and_module_from_path(rel_path: &str) -> (String, String) {
    let parts: Vec<&str> = rel_path.split('/').collect();

    if let Some(pos) = parts.iter().position(|&p| p == "crates") {
        if let Some(crate_name) = parts.get(pos + 1) {
            let src_idx = pos + 3;
            if parts.get(pos + 2) == Some(&"src") && parts.len() > src_idx {
                let module_parts: Vec<&str> = parts[src_idx..]
                    .iter()
                    .map(|p| p.trim_end_matches(".rs"))
                    .filter(|p| *p != "lib" && *p != "main")
                    .collect();
                return (crate_name.to_string(), module_parts.join("::"));
            }
            return (crate_name.to_string(), String::new());
        }
    }

    let stem = parts.last().unwrap_or(&"unknown").trim_end_matches(".rs");
    (stem.to_string(), String::new())
}

/// Scan file content for `// spec: <path>` annotations.
fn extract_spec_comments(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("// spec:") {
                let path = rest.trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
            None
        })
        .collect()
}

/// Extract doc comment text from syn attributes.
fn extract_doc_comment(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let syn::Meta::NameValue(nv) = &attr.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    lines.push(s.value().trim().to_string());
                }
            }
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Convert syn `Visibility` to domain `Visibility`.
fn syn_vis_to_visibility(vis: &syn::Visibility) -> Visibility {
    match vis {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Restricted(r) => {
            let path_str = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if path_str == "crate" {
                Visibility::PubCrate
            } else {
                Visibility::Private
            }
        }
        syn::Visibility::Inherited => Visibility::Private,
    }
}

fn format_fn_sig(sig: &syn::Signature) -> String {
    let inputs: Vec<String> = sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Receiver(r) => {
                if r.mutability.is_some() {
                    "&mut self".to_string()
                } else if r.reference.is_some() {
                    "&self".to_string()
                } else {
                    "self".to_string()
                }
            }
            syn::FnArg::Typed(pat) => {
                format!("{}: {}", pat_to_string(&pat.pat), type_to_string(&pat.ty))
            }
        })
        .collect();
    let ret = match &sig.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, ty) => format!(" -> {}", type_to_string(ty)),
    };
    format!("fn {}({}){}", sig.ident, inputs.join(", "), ret)
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn pat_to_string(pat: &syn::Pat) -> String {
    match pat {
        syn::Pat::Ident(i) => i.ident.to_string(),
        _ => "_".to_string(),
    }
}

fn type_to_string(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => path_to_string(&p.path),
        syn::Type::Reference(r) => {
            let inner = type_to_string(&r.elem);
            if r.mutability.is_some() {
                format!("&mut {inner}")
            } else {
                format!("&{inner}")
            }
        }
        _ => "?".to_string(),
    }
}

/// Parse `.route("/path", METHOD(handler))` from a line of code.
fn parse_route_line(line: &str) -> Option<(String, String, String)> {
    let after_route = line.find(".route(")?;
    let rest = &line[after_route + 7..];

    let path_start = rest.find('"')? + 1;
    let path_end = rest[path_start..].find('"')? + path_start;
    let route_path = rest[path_start..path_end].to_string();

    let after_path = &rest[path_end + 1..];
    let methods = ["get", "post", "put", "delete", "patch", "options", "head"];
    for method in &methods {
        let method_prefix = format!(", {method}(");
        if let Some(pos) = after_path.find(method_prefix.as_str()) {
            let handler_start = pos + method_prefix.len();
            let handler_end = after_path[handler_start..].find(')')? + handler_start;
            let handler = after_path[handler_start..handler_end].to_string();
            return Some((route_path, method.to_string(), handler));
        }
    }
    None
}

/// Extract table name from a `table! {` line.
fn extract_table_name_from_line(line: &str) -> Option<String> {
    let stripped = line
        .trim_start_matches("diesel::")
        .trim_start_matches("table!")
        .trim()
        .trim_start_matches('{')
        .trim();

    if stripped.is_empty() || stripped.starts_with("//") {
        return None;
    }

    let name: String = stripped
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_tempdir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn detect_returns_true_when_cargo_toml_exists() {
        let dir = make_tempdir();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        assert!(RustExtractor.detect(dir.path()));
    }

    #[test]
    fn detect_returns_false_without_cargo_toml() {
        let dir = make_tempdir();
        assert!(!RustExtractor.detect(dir.path()));
    }

    #[test]
    fn extract_package_node_from_cargo_toml() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "abc123");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Package && n.qualified_name == "my-crate"),
            "should have Package node for my-crate"
        );
    }

    #[test]
    fn extract_struct_trait_impl_fn() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
/// A user.
pub struct User {
    pub id: String,
    pub name: String,
}

pub enum Status { Active, Inactive }

pub trait Repository {
    fn find(&self, id: &str) -> Option<User>;
}

pub struct InMemoryRepo;
impl Repository for InMemoryRepo {
    fn find(&self, _id: &str) -> Option<User> { None }
}

pub fn create_user(name: &str) -> User {
    User { id: "1".to_string(), name: name.to_string() }
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "deadbeef");
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
            "should extract User struct"
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Type && n.name == "Status"),
            "should extract Status enum"
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Interface && n.name == "Repository"),
            "should extract Repository trait"
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Function && n.name == "create_user"),
            "should extract pub create_user fn"
        );
        assert!(
            result
                .edges
                .iter()
                .any(|e| e.edge_type == EdgeType::Implements),
            "should have Implements edge"
        );
    }

    #[test]
    fn extract_axum_endpoints() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"server\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let code = r#"
pub fn app() -> Router {
    Router::new()
        .route("/api/v1/agents", get(list_agents))
        .route("/api/v1/agents", post(create_agent))
        .route("/api/v1/agents/:id", delete(delete_agent))
}
"#;
        fs::write(src_dir.join("main.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "cafebabe");
        let endpoint_count = result
            .nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Endpoint)
            .count();
        assert!(
            endpoint_count >= 3,
            "should extract 3 endpoint nodes, got {endpoint_count}"
        );
    }

    #[test]
    fn extract_diesel_table_macro() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"adapters\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let schema = r#"
diesel::table! {
    agents (id) {
        id -> Text,
        name -> Text,
        status -> Text,
    }
}
"#;
        fs::write(src_dir.join("schema.rs"), schema).unwrap();

        let result = RustExtractor.extract(dir.path(), "deadbeef");
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Table && n.qualified_name == "agents"),
            "should extract 'agents' table node"
        );
    }

    #[test]
    fn extract_calls_edges() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            "pub fn caller() { callee(); }\npub fn callee() -> i32 { 42 }\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "abc123");
        assert!(result.errors.is_empty());
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();
        assert!(!calls_edges.is_empty(), "should have Calls edges");
    }

    #[test]
    fn extract_calls_edges_deduplicates() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            "pub fn caller() { callee(); callee(); callee(); }\npub fn callee() -> i32 { 42 }\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "abc123");
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();
        assert_eq!(calls_edges.len(), 1, "should deduplicate Calls edges");
    }

    #[test]
    fn extract_spec_governance_comment() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"domain\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let code = "// spec: specs/system/agent-gates.md\npub struct Gate {}\n";
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "aabbccdd");

        // Module node should have spec_path set.
        let module_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Module && n.file_path.ends_with("lib.rs"));
        assert!(module_node.is_some(), "should have module node for lib.rs");
        let module = module_node.unwrap();
        assert_eq!(
            module.spec_path.as_deref(),
            Some("specs/system/agent-gates.md"),
            "module should have spec_path set"
        );
        assert_eq!(module.spec_confidence, SpecConfidence::High);

        // GovernedBy edge should exist.
        assert!(
            result
                .edges
                .iter()
                .any(|e| e.edge_type == EdgeType::GovernedBy),
            "should have GovernedBy edge"
        );
    }
}
