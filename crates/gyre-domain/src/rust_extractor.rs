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
            import_aliases: HashMap::new(),
            unresolved_calls: Vec::new(),
            deferred_returns: Vec::new(),
            workspace_crates: HashSet::new(),
        };

        ctx.extract_packages();
        // Record workspace crate names before file extraction so use-resolution
        // can distinguish intra-workspace imports from external ones.
        ctx.workspace_crates = ctx.name_to_id.keys().cloned().collect();
        ctx.extract_rust_files();
        ctx.resolve_calls();
        ctx.resolve_returns();

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

/// An unresolved call site: the calling function's qualified name and the
/// callee's short name (as written in source code).
#[derive(Debug, Clone)]
struct UnresolvedCall {
    /// Qualified name of the calling function.
    caller_qname: String,
    /// Short name of the callee as it appears in source (e.g. `foo`, `bar`).
    callee_name: String,
    /// Whether this is a method call (`x.bar()`) vs a function call (`bar()`).
    is_method: bool,
}

struct ExtractionContext {
    repo_root: PathBuf,
    commit_sha: String,
    now: u64,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    errors: Vec<ExtractionError>,
    /// Map qualified name → node Id for edge resolution.
    name_to_id: HashMap<String, Id>,
    /// Per-module import aliases: (module_qname, short_name) → qualified_name.
    import_aliases: HashMap<(String, String), String>,
    /// Unresolved call sites collected during the first pass.
    unresolved_calls: Vec<UnresolvedCall>,
    /// Deferred Returns edges: (function_node_id, return_type_name) to resolve after extraction.
    deferred_returns: Vec<(Id, String)>,
    /// Set of known workspace crate names (from Cargo.toml [package] names).
    workspace_crates: HashSet<String>,
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
            spec_paths: vec![],
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

        let mut module_node = self.make_node(
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
        // Tag modules in tests/ directories or named "tests" as test nodes.
        if is_test_file_path(&rel_path) || module_short_name == "tests" {
            module_node.test_node = true;
        }
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

        // Extract use statements for import alias resolution.
        self.extract_use_statements(&syntax, &crate_name, &module_qname);

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

        // Collect unresolved call sites from function bodies.
        let mut call_collector = CallCollector {
            module_qname: module_qname.clone(),
            current_fn_qname: None,
            calls: Vec::new(),
        };
        call_collector.visit_file(&syntax);
        self.unresolved_calls.extend(call_collector.calls);

        self.extract_endpoints_from_text(&content, &rel_path, &module_id);
        self.extract_diesel_tables(&content, &rel_path, &module_id);

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

    // -----------------------------------------------------------------------
    // Use statement extraction
    // -----------------------------------------------------------------------

    /// Walk all `use` items in a parsed file and record import aliases.
    fn extract_use_statements(&mut self, syntax: &syn::File, crate_name: &str, module_qname: &str) {
        for item in &syntax.items {
            if let Item::Use(use_item) = item {
                let mut aliases = Vec::new();
                collect_use_tree(&use_item.tree, &[], &mut aliases);

                for (short_name, segments) in aliases {
                    // Resolve `crate::` and `super::` prefixes.
                    let qualified = self.resolve_use_path(&segments, crate_name, module_qname);
                    if let Some(qname) = qualified {
                        self.import_aliases
                            .insert((module_qname.to_string(), short_name), qname);
                    }
                }
            }
        }
    }

    /// Resolve a use path's segments into a qualified name, or None if external.
    fn resolve_use_path(
        &self,
        segments: &[String],
        crate_name: &str,
        module_qname: &str,
    ) -> Option<String> {
        if segments.is_empty() {
            return None;
        }

        let first = &segments[0];

        if first == "crate" {
            // crate:: → replace with crate_name
            let mut parts = vec![crate_name.to_string()];
            parts.extend(segments[1..].iter().cloned());
            return Some(parts.join("::"));
        }

        if first == "super" {
            // super:: → go up one module from current module_qname
            let parent = module_qname
                .rsplit_once("::")
                .map(|(p, _)| p)
                .unwrap_or(crate_name);
            let mut parts = vec![parent.to_string()];
            // Handle chained super::super::
            let mut remaining_start = 1;
            for seg in &segments[1..] {
                if seg == "super" {
                    let p = parts.last().unwrap().clone();
                    let grandparent = p
                        .rsplit_once("::")
                        .map(|(pp, _)| pp.to_string())
                        .unwrap_or_else(|| crate_name.to_string());
                    *parts.last_mut().unwrap() = grandparent;
                    remaining_start += 1;
                } else {
                    break;
                }
            }
            parts.extend(segments[remaining_start..].iter().cloned());
            return Some(parts.join("::"));
        }

        if first == "self" {
            // self:: → current module
            let mut parts = vec![module_qname.to_string()];
            parts.extend(segments[1..].iter().cloned());
            return Some(parts.join("::"));
        }

        // Check if first segment is a workspace crate (using underscore-normalized name).
        let normalized = first.replace('-', "_");
        let is_workspace = self
            .workspace_crates
            .iter()
            .any(|c| c == first || c.replace('-', "_") == normalized);

        if is_workspace {
            // Find the actual crate name (might be hyphenated).
            let actual_crate = self
                .workspace_crates
                .iter()
                .find(|c| *c == first || c.replace('-', "_") == normalized)
                .cloned()
                .unwrap_or_else(|| first.clone());
            let mut parts = vec![actual_crate];
            parts.extend(segments[1..].iter().cloned());
            return Some(parts.join("::"));
        }

        // External crate (std, tokio, serde, etc.) — skip.
        None
    }

    // -----------------------------------------------------------------------
    // Cross-module call resolution (second pass)
    // -----------------------------------------------------------------------

    fn resolve_calls(&mut self) {
        let calls = std::mem::take(&mut self.unresolved_calls);
        let mut seen_edges: HashSet<(String, String)> = HashSet::new();

        // Collect existing Calls edges to avoid duplicates.
        for edge in &self.edges {
            if edge.edge_type == EdgeType::Calls {
                seen_edges.insert((edge.source_id.to_string(), edge.target_id.to_string()));
            }
        }

        // Build a suffix index: last segment of qualified name → list of Ids.
        // This helps resolve method calls where we only know the method name.
        let mut suffix_index: HashMap<String, Vec<(String, Id)>> = HashMap::new();
        for (qname, id) in &self.name_to_id {
            if let Some(last) = qname.rsplit("::").next() {
                suffix_index
                    .entry(last.to_string())
                    .or_default()
                    .push((qname.clone(), id.clone()));
            }
        }

        for call in &calls {
            let caller_id = match self.name_to_id.get(&call.caller_qname) {
                Some(id) => id.clone(),
                None => continue,
            };

            // Extract the module from the caller's qualified name.
            let caller_module = call
                .caller_qname
                .rsplit_once("::")
                .map(|(m, _)| m.to_string())
                .unwrap_or_default();

            let callee_id = self.resolve_callee(
                &call.callee_name,
                &caller_module,
                call.is_method,
                &suffix_index,
            );

            if let Some(target_id) = callee_id {
                let key = (caller_id.to_string(), target_id.to_string());
                if seen_edges.insert(key) {
                    let edge = self.make_edge(EdgeType::Calls, caller_id, target_id);
                    self.edges.push(edge);
                }
            }
        }
    }

    /// Try to resolve a callee name to a node Id.
    fn resolve_callee(
        &self,
        callee_name: &str,
        caller_module: &str,
        is_method: bool,
        suffix_index: &HashMap<String, Vec<(String, Id)>>,
    ) -> Option<Id> {
        // For path-qualified calls like `module::func`, try the last segment
        // for suffix matching but also try the full path.
        let short_name = callee_name.rsplit("::").next().unwrap_or(callee_name);

        // 1. Try same-module lookup: caller_module::callee_name
        if !caller_module.is_empty() {
            let same_module_qname = format!("{caller_module}::{short_name}");
            if let Some(id) = self.name_to_id.get(&same_module_qname) {
                return Some(id.clone());
            }
        }

        // 2. Try import alias resolution.
        if !caller_module.is_empty() {
            let alias_key = (caller_module.to_string(), short_name.to_string());
            if let Some(resolved_qname) = self.import_aliases.get(&alias_key) {
                if let Some(id) = self.name_to_id.get(resolved_qname) {
                    return Some(id.clone());
                }
            }

            // For path-qualified calls like `foo::bar`, try resolving `foo` as
            // an alias and then appending `bar`.
            if callee_name.contains("::") {
                let parts: Vec<&str> = callee_name.splitn(2, "::").collect();
                let prefix_alias_key = (caller_module.to_string(), parts[0].to_string());
                if let Some(resolved_prefix) = self.import_aliases.get(&prefix_alias_key) {
                    let full_qname = format!("{resolved_prefix}::{}", parts[1]);
                    if let Some(id) = self.name_to_id.get(&full_qname) {
                        return Some(id.clone());
                    }
                }
            }
        }

        // 3. Direct global lookup.
        if let Some(id) = self.name_to_id.get(callee_name) {
            return Some(id.clone());
        }

        // 4. For method calls or simple names, try suffix match.
        // Only match if exactly one candidate (to avoid ambiguity).
        if is_method || !callee_name.contains("::") {
            if let Some(candidates) = suffix_index.get(short_name) {
                if candidates.len() == 1 {
                    return Some(candidates[0].1.clone());
                }
            }
        }

        None
    }

    /// Resolve deferred Returns edges: function → return type.
    fn resolve_returns(&mut self) {
        let returns = std::mem::take(&mut self.deferred_returns);
        let mut seen = HashSet::new();
        for (fn_id, type_name) in returns {
            // Try to find the return type in name_to_id
            // First try exact match, then suffix match
            let target_id = self.name_to_id.get(&type_name).cloned().or_else(|| {
                // Suffix match: find any node whose qualified name ends with ::type_name
                let suffix = format!("::{}", type_name);
                self.name_to_id
                    .iter()
                    .find(|(k, _)| k.ends_with(&suffix) || *k == &type_name)
                    .map(|(_, v)| v.clone())
            });
            if let Some(target_id) = target_id {
                let key = (fn_id.to_string(), target_id.to_string());
                if seen.insert(key) {
                    let edge = GraphEdge {
                        id: Self::new_id(),
                        repo_id: Self::placeholder_repo_id(),
                        source_id: fn_id,
                        target_id,
                        edge_type: EdgeType::Returns,
                        metadata: None,
                        first_seen_at: self.now,
                        last_seen_at: self.now,
                        deleted_at: None,
                    };
                    self.edges.push(edge);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Call site collector — walks function bodies to find calls
// ---------------------------------------------------------------------------

struct CallCollector {
    module_qname: String,
    /// Qualified name of the function we're currently inside.
    current_fn_qname: Option<String>,
    /// Collected unresolved calls.
    calls: Vec<UnresolvedCall>,
}

impl<'ast> Visit<'ast> for CallCollector {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let name = f.sig.ident.to_string();
        let prev = self.current_fn_qname.take();
        self.current_fn_qname = Some(format!("{}::{}", self.module_qname, name));
        syn::visit::visit_item_fn(self, f);
        self.current_fn_qname = prev;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let name = f.sig.ident.to_string();
        let prev = self.current_fn_qname.take();
        self.current_fn_qname = Some(format!("{}::{}", self.module_qname, name));
        syn::visit::visit_impl_item_fn(self, f);
        self.current_fn_qname = prev;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some(ref caller_qname) = self.current_fn_qname {
            if let Some(callee_name) = extract_call_name(&call.func) {
                self.calls.push(UnresolvedCall {
                    caller_qname: caller_qname.clone(),
                    callee_name,
                    is_method: false,
                });
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if let Some(ref caller_qname) = self.current_fn_qname {
            self.calls.push(UnresolvedCall {
                caller_qname: caller_qname.clone(),
                callee_name: call.method.to_string(),
                is_method: true,
            });
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// Extract the callee name from a call expression's function position.
fn extract_call_name(func: &syn::Expr) -> Option<String> {
    match func {
        syn::Expr::Path(ep) => Some(path_to_string(&ep.path)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Use tree walker
// ---------------------------------------------------------------------------

/// Recursively collect all (short_name, full_path_segments) pairs from a use tree.
fn collect_use_tree(tree: &syn::UseTree, prefix: &[String], out: &mut Vec<(String, Vec<String>)>) {
    match tree {
        syn::UseTree::Path(p) => {
            let mut new_prefix = prefix.to_vec();
            new_prefix.push(p.ident.to_string());
            collect_use_tree(&p.tree, &new_prefix, out);
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            let mut segments = prefix.to_vec();
            segments.push(name.clone());
            out.push((name, segments));
        }
        syn::UseTree::Rename(r) => {
            let alias = r.rename.to_string();
            let mut segments = prefix.to_vec();
            segments.push(r.ident.to_string());
            out.push((alias, segments));
        }
        syn::UseTree::Glob(_) => {
            // `use foo::*;` — we skip glob imports as they're hard to resolve
            // without type information.
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                collect_use_tree(item, prefix, out);
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
                let node = self.make_node(NodeType::Type, &name, 0, vis.clone(), doc);
                let node_id = node.id.clone();
                let struct_qname = self.qualified(&name);
                self.ctx
                    .name_to_id
                    .insert(struct_qname.clone(), node_id.clone());
                let edge = self.add_contains_edge(node_id.clone());
                self.new_nodes.push(node);
                self.new_edges.push(edge);

                // Extract fields from public structs with named fields (skip >50 fields).
                if matches!(vis, Visibility::Public) {
                    if let syn::Fields::Named(fields) = &s.fields {
                        if fields.named.len() <= 50 {
                            for field in &fields.named {
                                if let Some(ident) = &field.ident {
                                    let field_name = ident.to_string();
                                    let field_qname = format!("{struct_qname}::{field_name}");
                                    let type_str = type_to_string(&field.ty);
                                    let field_node = self.ctx.make_node(
                                        NodeType::Field,
                                        &field_name,
                                        &field_qname,
                                        self.rel_path,
                                        0,
                                        0,
                                        Visibility::Public,
                                        Some(type_str.clone()),
                                        None,
                                        SpecConfidence::None,
                                    );
                                    let field_id = field_node.id.clone();
                                    self.ctx.name_to_id.insert(field_qname, field_id.clone());
                                    self.new_nodes.push(field_node);

                                    // FieldOf edge: field → parent struct
                                    let field_edge = self.ctx.make_edge(
                                        EdgeType::FieldOf,
                                        field_id.clone(),
                                        node_id.clone(),
                                    );
                                    self.new_edges.push(field_edge);

                                    // DependsOn edge if field type refers to a known type
                                    let bare_type = type_str
                                        .trim_start_matches('&')
                                        .trim_start_matches("mut ")
                                        .to_string();
                                    // Try the type as-is and also qualified in current module
                                    let candidates = [
                                        self.ctx.name_to_id.get(&bare_type).cloned(),
                                        self.ctx
                                            .name_to_id
                                            .get(&self.qualified(&bare_type))
                                            .cloned(),
                                    ];
                                    for candidate in candidates.iter().flatten() {
                                        let dep_edge = self.ctx.make_edge(
                                            EdgeType::DependsOn,
                                            field_id.clone(),
                                            candidate.clone(),
                                        );
                                        self.new_edges.push(dep_edge);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Item::Enum(e) => {
                let name = e.ident.to_string();
                let vis = syn_vis_to_visibility(&e.vis);
                let doc = extract_doc_comment(&e.attrs);
                let is_subcommand = has_derive_attr(&e.attrs, "Subcommand");

                let node = self.make_node(NodeType::Enum, &name, 0, vis, doc);
                let node_id = node.id.clone();
                let enum_qname = self.qualified(&name);
                self.ctx
                    .name_to_id
                    .insert(enum_qname.clone(), node_id.clone());
                let edge = self.add_contains_edge(node_id);
                self.new_nodes.push(node);
                self.new_edges.push(edge);

                // If this enum derives Subcommand, emit each variant as an Endpoint.
                if is_subcommand {
                    for variant in &e.variants {
                        let variant_name = variant.ident.to_string();
                        let variant_qname = format!("{}::{}", enum_qname, variant_name);
                        let variant_doc = extract_doc_comment(&variant.attrs);
                        // Convert CamelCase variant to kebab-case for the command name
                        let cmd_name = camel_to_kebab(&variant_name);
                        let ep_node = self.ctx.make_node(
                            NodeType::Endpoint,
                            &cmd_name,
                            &variant_qname,
                            self.rel_path,
                            0,
                            0,
                            Visibility::Public,
                            variant_doc,
                            None,
                            SpecConfidence::None,
                        );
                        let ep_id = ep_node.id.clone();
                        self.ctx.name_to_id.insert(variant_qname, ep_id.clone());
                        let ep_edge =
                            self.ctx
                                .make_edge(EdgeType::Contains, self.module_id.clone(), ep_id);
                        self.new_nodes.push(ep_node);
                        self.new_edges.push(ep_edge);
                    }
                }
            }
            Item::Trait(t) => {
                let name = t.ident.to_string();
                let vis = syn_vis_to_visibility(&t.vis);
                let doc = extract_doc_comment(&t.attrs);
                let node = self.make_node(NodeType::Trait, &name, 0, vis, doc);
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
                let is_test_fn = has_test_attr(&f.attrs);
                let is_in_test_context =
                    is_test_file_path(self.rel_path) || self.module_qname.contains("::tests");
                if matches!(f.vis, syn::Visibility::Public(_)) || is_test_fn {
                    let name = f.sig.ident.to_string();
                    let doc = extract_doc_comment(&f.attrs);
                    let sig = format_fn_sig(&f.sig);
                    let vis = syn_vis_to_visibility(&f.vis);
                    let mut node = self.make_node(NodeType::Function, &name, 0, vis, doc);
                    if node.doc_comment.is_none() {
                        node.doc_comment = Some(sig);
                    }
                    node.test_node = is_test_fn || is_in_test_context;
                    let node_id = node.id.clone();
                    self.ctx
                        .name_to_id
                        .insert(self.qualified(&name), node_id.clone());
                    let edge = self.add_contains_edge(node_id.clone());
                    self.new_nodes.push(node);
                    self.new_edges.push(edge);
                    // Emit Returns edge for the return type (if it's a named type)
                    if let syn::ReturnType::Type(_, ty) = &f.sig.output {
                        let ret_type_name = type_to_string(ty);
                        // Strip references and generics wrapper for resolution
                        let base_name = ret_type_name
                            .trim_start_matches('&')
                            .trim_start_matches("mut ")
                            .split('<')
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !base_name.is_empty() && base_name != "?" {
                            self.ctx
                                .deferred_returns
                                .push((node_id, base_name.to_string()));
                        }
                    }
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

/// Check if a function has a `#[test]` or `#[tokio::test]` attribute.
fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        let path = attr.path();
        // #[test]
        if path.is_ident("test") {
            return true;
        }
        // #[tokio::test], #[async_std::test], etc.
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if segments.last().map(|s| s.as_str()) == Some("test") {
            return true;
        }
    }
    false
}

/// Check if a file path indicates a test file (in a `tests/` directory).
fn is_test_file_path(rel_path: &str) -> bool {
    let parts: Vec<&str> = rel_path.split('/').collect();
    parts.iter().any(|&p| p == "tests")
}

/// Check if an item has a `#[derive(...)]` attribute containing the given trait name.
fn has_derive_attr(attrs: &[syn::Attribute], trait_name: &str) -> bool {
    for attr in attrs {
        if attr.path().is_ident("derive") {
            // Parse the derive list: #[derive(Foo, Bar)]
            if let syn::Meta::List(list) = &attr.meta {
                let tokens_str = list.tokens.to_string();
                // Check each comma-separated token
                for token in tokens_str.split(',') {
                    let trimmed = token.trim();
                    // Handle both `Subcommand` and `clap::Subcommand`
                    let last_segment = trimmed.rsplit("::").next().unwrap_or(trimmed);
                    if last_segment == trait_name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Convert a CamelCase name to kebab-case for CLI command naming.
/// e.g. `InitProject` → `init-project`, `Clone` → `clone`
fn camel_to_kebab(name: &str) -> String {
    let mut result = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
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
        .map(|s| {
            let ident = s.ident.to_string();
            match &s.arguments {
                syn::PathArguments::None => ident,
                syn::PathArguments::AngleBracketed(args) => {
                    let inner: Vec<String> = args
                        .args
                        .iter()
                        .map(|arg| match arg {
                            syn::GenericArgument::Type(ty) => type_to_string(ty),
                            syn::GenericArgument::Lifetime(lt) => format!("'{}", lt.ident),
                            _ => "_".to_string(),
                        })
                        .collect();
                    format!("{}<{}>", ident, inner.join(", "))
                }
                syn::PathArguments::Parenthesized(args) => {
                    let inputs: Vec<String> = args.inputs.iter().map(type_to_string).collect();
                    format!("{}({})", ident, inputs.join(", "))
                }
            }
        })
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
                .any(|n| n.node_type == NodeType::Enum && n.name == "Status"),
            "should extract Status enum"
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|n| n.node_type == NodeType::Trait && n.name == "Repository"),
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

    #[test]
    fn extract_intra_module_calls() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
pub fn helper() -> String {
    "hello".to_string()
}

pub fn caller() -> String {
    helper()
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "abc123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Both functions should exist as nodes.
        let helper_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "helper");
        let caller_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "caller");
        assert!(helper_node.is_some(), "should have helper function node");
        assert!(caller_node.is_some(), "should have caller function node");

        // There should be a Calls edge from caller → helper.
        let caller_id = &caller_node.unwrap().id;
        let helper_id = &helper_node.unwrap().id;
        let has_call_edge = result.edges.iter().any(|e| {
            e.edge_type == EdgeType::Calls && e.source_id == *caller_id && e.target_id == *helper_id
        });
        assert!(
            has_call_edge,
            "should have Calls edge from caller to helper"
        );
    }

    #[test]
    fn extract_cross_crate_calls_via_use() {
        // Simulate two crates: `crate-a` exports a function, `crate-b` imports and calls it.
        let dir = make_tempdir();

        // Workspace Cargo.toml
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/crate-a\", \"crates/crate-b\"]\n",
        )
        .unwrap();

        // crate-a
        let crate_a = dir.path().join("crates/crate-a");
        fs::create_dir_all(crate_a.join("src")).unwrap();
        fs::write(
            crate_a.join("Cargo.toml"),
            "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            crate_a.join("src/lib.rs"),
            "pub fn shared_helper() -> u32 { 42 }\n",
        )
        .unwrap();

        // crate-b depends on crate-a
        let crate_b = dir.path().join("crates/crate-b");
        fs::create_dir_all(crate_b.join("src")).unwrap();
        fs::write(
            crate_b.join("Cargo.toml"),
            "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n\n[dependencies]\ncrate-a = { path = \"../crate-a\" }\n",
        )
        .unwrap();
        fs::write(
            crate_b.join("src/lib.rs"),
            "use crate_a::shared_helper;\n\npub fn consumer() -> u32 {\n    shared_helper()\n}\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "cross123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Both functions should exist.
        let helper_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "shared_helper");
        let consumer_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "consumer");
        assert!(helper_node.is_some(), "should have shared_helper node");
        assert!(consumer_node.is_some(), "should have consumer node");

        // Calls edge from consumer → shared_helper.
        let consumer_id = &consumer_node.unwrap().id;
        let helper_id = &helper_node.unwrap().id;
        let has_call = result.edges.iter().any(|e| {
            e.edge_type == EdgeType::Calls
                && e.source_id == *consumer_id
                && e.target_id == *helper_id
        });
        assert!(
            has_call,
            "should have Calls edge from consumer to shared_helper (cross-crate)"
        );
    }

    #[test]
    fn extract_group_use_imports() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/lib-a\", \"crates/lib-b\"]\n",
        )
        .unwrap();

        // lib-a with two public functions
        let lib_a = dir.path().join("crates/lib-a");
        fs::create_dir_all(lib_a.join("src")).unwrap();
        fs::write(
            lib_a.join("Cargo.toml"),
            "[package]\nname = \"lib-a\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            lib_a.join("src/lib.rs"),
            "pub fn alpha() -> u32 { 1 }\npub fn beta() -> u32 { 2 }\n",
        )
        .unwrap();

        // lib-b imports both via group use
        let lib_b = dir.path().join("crates/lib-b");
        fs::create_dir_all(lib_b.join("src")).unwrap();
        fs::write(
            lib_b.join("Cargo.toml"),
            "[package]\nname = \"lib-b\"\nversion = \"0.1.0\"\n\n[dependencies]\nlib-a = { path = \"../lib-a\" }\n",
        )
        .unwrap();
        fs::write(
            lib_b.join("src/lib.rs"),
            "use lib_a::{alpha, beta};\n\npub fn combined() -> u32 {\n    alpha() + beta()\n}\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "group123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let combined_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "combined");
        assert!(combined_node.is_some(), "should have combined node");
        let combined_id = &combined_node.unwrap().id;

        let alpha_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "alpha");
        let beta_node = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "beta");
        assert!(alpha_node.is_some(), "should have alpha node");
        assert!(beta_node.is_some(), "should have beta node");

        // Should have Calls edges to both alpha and beta.
        let calls_alpha = result.edges.iter().any(|e| {
            e.edge_type == EdgeType::Calls
                && e.source_id == *combined_id
                && e.target_id == alpha_node.unwrap().id
        });
        let calls_beta = result.edges.iter().any(|e| {
            e.edge_type == EdgeType::Calls
                && e.source_id == *combined_id
                && e.target_id == beta_node.unwrap().id
        });
        assert!(calls_alpha, "should have Calls edge to alpha");
        assert!(calls_beta, "should have Calls edge to beta");
    }

    #[test]
    fn calls_edges_are_deduplicated() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"dedup\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Call helper() twice in the same function — should produce only one edge.
        let code = r#"
pub fn helper() -> u32 { 1 }

pub fn caller() -> u32 {
    helper() + helper()
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "dedup123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();
        assert_eq!(
            call_edges.len(),
            1,
            "should have exactly 1 Calls edge (deduplicated), got {}",
            call_edges.len()
        );
    }

    #[test]
    fn crate_relative_use_resolves() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir_all(src_dir.join("utils")).unwrap();

        // Define a function in a submodule.
        fs::write(
            src_dir.join("utils/helpers.rs"),
            "pub fn utility() -> u32 { 99 }\n",
        )
        .unwrap();

        // Import it with `use crate::utils::helpers::utility;`
        fs::write(
            src_dir.join("lib.rs"),
            "mod utils;\nuse crate::utils::helpers::utility;\n\npub fn main_fn() -> u32 {\n    utility()\n}\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "crate-rel");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        let main_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "main_fn");
        let utility_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "utility");
        assert!(main_fn.is_some(), "should have main_fn node");
        assert!(utility_fn.is_some(), "should have utility node");

        let has_call = result.edges.iter().any(|e| {
            e.edge_type == EdgeType::Calls
                && e.source_id == main_fn.unwrap().id
                && e.target_id == utility_fn.unwrap().id
        });
        assert!(
            has_call,
            "should have Calls edge from main_fn to utility via crate:: import"
        );
    }

    #[test]
    fn extract_struct_fields_as_field_of_edges() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
pub struct Config {
    pub host: String,
    pub port: u16,
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "field123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Should have Field nodes for host and port.
        let host_field = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Field && n.name == "host");
        let port_field = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Field && n.name == "port");
        assert!(host_field.is_some(), "should extract host field");
        assert!(port_field.is_some(), "should extract port field");

        // doc_comment should contain the type annotation.
        assert_eq!(
            host_field.unwrap().doc_comment.as_deref(),
            Some("String"),
            "host field doc_comment should be the type"
        );

        // Should have FieldOf edges.
        let field_of_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::FieldOf)
            .collect();
        assert_eq!(
            field_of_edges.len(),
            2,
            "should have 2 FieldOf edges, got {}",
            field_of_edges.len()
        );
    }

    #[test]
    fn external_crate_use_not_resolved() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
use std::collections::HashMap;

pub fn make_map() -> u32 {
    let _m = HashMap::new();
    42
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "ext123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // No Calls edges should be produced for std::HashMap::new.
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();
        assert!(
            call_edges.is_empty(),
            "should not produce Calls edges for external crate functions, got {}",
            call_edges.len()
        );
    }

    #[test]
    fn test_functions_tagged_as_test_nodes() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
pub fn production_code() -> u32 { 42 }

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
"#;
        fs::write(src_dir.join("lib.rs"), code).unwrap();

        // Also add a file in tests/ directory.
        let tests_dir = dir.path().join("tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(
            tests_dir.join("integration.rs"),
            "pub fn integration_helper() {}\n",
        )
        .unwrap();

        let result = RustExtractor.extract(dir.path(), "test123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // The #[test] function should be tagged as test_node.
        let test_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "it_works");
        assert!(test_fn.is_some(), "should extract #[test] fn it_works");
        assert!(
            test_fn.unwrap().test_node,
            "it_works should be tagged as test_node"
        );

        // The production function should NOT be tagged as test_node.
        let prod_fn = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Function && n.name == "production_code");
        assert!(prod_fn.is_some(), "should extract production_code");
        assert!(
            !prod_fn.unwrap().test_node,
            "production_code should NOT be tagged as test_node"
        );

        // Module in tests/ directory should be tagged.
        let test_module = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Module && n.file_path.contains("tests/"));
        assert!(
            test_module.is_some(),
            "should have module node for tests/ file"
        );
        assert!(
            test_module.unwrap().test_node,
            "module in tests/ should be tagged as test_node"
        );
    }

    #[test]
    fn extract_clap_subcommand_enum_variants_as_endpoints() {
        let dir = make_tempdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let code = r#"
use clap::{Parser, Subcommand};

#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new project
    Init {
        path: String,
    },
    /// Clone an existing repository
    Clone {
        url: String,
    },
}
"#;
        fs::write(src_dir.join("main.rs"), code).unwrap();

        let result = RustExtractor.extract(dir.path(), "clap123");
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );

        // Should have Endpoint nodes for each variant.
        let init_ep = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "init");
        let clone_ep = result
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Endpoint && n.name == "clone");

        assert!(
            init_ep.is_some(),
            "should extract Init variant as Endpoint with name 'init'"
        );
        assert!(
            clone_ep.is_some(),
            "should extract Clone variant as Endpoint with name 'clone'"
        );

        // Verify doc comments are preserved.
        assert_eq!(
            init_ep.unwrap().doc_comment.as_deref(),
            Some("Initialize a new project"),
            "Init endpoint should have doc comment"
        );
    }
}
