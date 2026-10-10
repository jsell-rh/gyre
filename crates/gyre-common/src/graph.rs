//! Knowledge graph domain types for the realized model (specs/system/realized-model.md).

use crate::Id;
use serde::{Deserialize, Serialize};

/// Universal node types in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Package,
    Module,
    Type,
    /// A trait or interface definition (distinct from Type for trait-level granularity).
    Trait,
    Interface,
    Function,
    /// A method on a type or trait (carries parent context).
    Method,
    /// A class definition (Python, TypeScript, Go struct with methods).
    Class,
    /// An enum definition.
    Enum,
    /// An enum variant.
    EnumVariant,
    Endpoint,
    Component,
    Table,
    Constant,
    Field,
    /// A specification document — first-class artifact per Vision Principle 3.
    Spec,
}

/// Typed relationship between two graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Contains,
    Implements,
    DependsOn,
    Calls,
    FieldOf,
    Returns,
    RoutesTo,
    Renders,
    PersistsTo,
    GovernedBy,
    ProducedBy,
}

/// Symbol visibility level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    PubCrate,
    Private,
}

/// Confidence that a node is governed by a specific spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecConfidence {
    High,
    Medium,
    Low,
    None,
}

/// A node in the knowledge graph representing a code symbol or architectural entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Id,
    pub repo_id: Id,
    pub node_type: NodeType,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub spec_path: Option<String>,
    /// Additional spec paths when a node is governed by multiple specs (N:M mapping).
    /// The primary spec is in `spec_path`; extras here. GovernedBy edges are canonical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spec_paths: Vec<String>,
    pub spec_confidence: SpecConfidence,
    pub last_modified_sha: String,
    pub last_modified_by: Option<Id>,
    pub last_modified_at: u64,
    pub created_sha: String,
    pub created_at: u64,
    pub complexity: Option<u32>,
    pub churn_count_30d: u32,
    /// Test coverage ratio (0.0–1.0). `None` when coverage data is unavailable.
    pub test_coverage: Option<f64>,
    /// Unix timestamp when this node first appeared in any extraction.
    pub first_seen_at: u64,
    /// Unix timestamp of the most recent extraction that included this node.
    pub last_seen_at: u64,
    /// Set when a node is no longer present in extraction (soft-delete). `None` = active.
    pub deleted_at: Option<u64>,
    /// Whether this node is a test function/class (for structural test coverage analysis).
    #[serde(default)]
    pub test_node: bool,
    /// When a spec was approved for this node (epoch seconds), if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_approved_at: Option<u64>,
    /// When a milestone was completed for this node (epoch seconds), if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone_completed_at: Option<u64>,
}

/// A directed edge between two graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: Id,
    pub repo_id: Id,
    pub source_id: Id,
    pub target_id: Id,
    pub edge_type: EdgeType,
    /// Optional JSON metadata for the edge.
    pub metadata: Option<String>,
    /// Unix timestamp when this edge first appeared in any extraction.
    pub first_seen_at: u64,
    /// Unix timestamp of the most recent extraction that included this edge.
    pub last_seen_at: u64,
    /// Set when an edge is no longer present in extraction (soft-delete). `None` = active.
    pub deleted_at: Option<u64>,
}

/// A single field change within a modified graph node (HSI §8 / realized-model.md §3).
///
/// Used in the divergence detection algorithm to compare how two agents modified
/// the same node.  `old_value` is informational only — conflicts are detected by
/// comparing `(field, new_value)` pairs between deltas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

/// Compact node identity used inside `delta_json` for divergence comparison.
///
/// Stored instead of the full `GraphNode` to keep delta records small.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeltaNodeEntry {
    pub name: String,
    pub node_type: String,
    pub qualified_name: String,
}

/// A recorded architectural change associated with a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalDelta {
    pub id: Id,
    pub repo_id: Id,
    pub commit_sha: String,
    pub timestamp: u64,
    pub agent_id: Option<Id>,
    pub spec_ref: Option<String>,
    /// Serialized delta details (JSON).
    ///
    /// Schema (when agent context is present):
    /// ```json
    /// {
    ///   "nodes_extracted": 5,
    ///   "edges_extracted": 3,
    ///   "nodes_added": [{"name":"Foo","node_type":"type","qualified_name":"crate::Foo"}],
    ///   "nodes_modified": []
    /// }
    /// ```
    /// Schema (no agent context — compact):
    /// ```json
    /// {"nodes_extracted": 5, "edges_extracted": 3}
    /// ```
    pub delta_json: String,
}

/// Risk metrics for a module derived from graph analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub module_name: String,
    pub churn_rate: f32,
    pub coupling_score: f32,
    pub spec_coverage: f32,
    pub complexity: f32,
    pub fan_in: u32,
    pub fan_out: u32,
    pub agent_contention: u32,
    pub staleness_days: u32,
}

/// A named concept view that groups related graph nodes for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptView {
    pub name: String,
    pub description: String,
    /// Glob patterns matching node qualified names to include.
    pub include_types: Vec<String>,
    pub include_traits: Vec<String>,
    pub include_modules: Vec<String>,
    pub include_endpoints: Vec<String>,
    pub include_specs: Vec<String>,
}

// ── Concept view projection (realized-model.md §4) ──────────────────────────

/// Match `text` against a `*`-glob `pattern`.
///
/// `*` matches any (possibly empty) run of characters; every other character
/// matches itself. Matching is case-sensitive and anchored at both ends:
/// `MergeRequest` does not match `MergeRequestDependency`, and `Gate*` does
/// not match `LateGate`. `*` is the only wildcard the concept-include grammar
/// defines (realized-model.md §4).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    // Dynamic programming over (pattern char, text char) with `*` splitting.
    // p[i..] matches t[j..] iff:
    //   - p[i] == '*'  and (p[i+1..] matches t[j..] or p[i..] matches t[j+1..])
    //   - p[i] == t[j] and p[i+1..] matches t[j+1..]
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (n, m) = (p.len(), t.len());
    // matches[i][j] = p[i..] matches t[j..]
    let mut matches = vec![vec![false; m + 1]; n + 1];
    matches[n][m] = true;
    // Trailing wildcard run can match the empty suffix.
    for i in (0..n).rev() {
        if p[i] == '*' {
            matches[i][m] = matches[i + 1][m];
        } else {
            break;
        }
    }
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            matches[i][j] = if p[i] == '*' {
                matches[i + 1][j] || matches[i][j + 1]
            } else {
                p[i] == t[j] && matches[i + 1][j + 1]
            };
        }
    }
    matches[0][0]
}

/// True when any pattern in `patterns` matches `text` (anchored glob).
fn glob_any(patterns: &[String], text: &str) -> bool {
    patterns.iter().any(|p| glob_match(p, text))
}

/// Normalize a spec path for concept `specs:` matching: strip a leading
/// `specs/` segment and directory prefix so `"identity-security.md"` matches
/// both `specs/identity-security.md` and `specs/system/identity-security.md`.
fn spec_file_name(spec_path: &str) -> &str {
    spec_path.rsplit('/').next().unwrap_or(spec_path)
}

/// Extract the HTTP route path from an edge's JSON metadata
/// (`{"path": "/api/v1/auth/login", ...}`).
fn edge_route_path(metadata: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(metadata).ok()?;
    value.get("path")?.as_str().map(str::to_string)
}

impl ConceptView {
    /// Project `nodes`/`edges` through this concept view (realized-model.md §4).
    ///
    /// A node is included when it matches ANY include rule (union):
    /// - `include_types`    — Type/Class/Enum nodes glob-matched on `name`
    ///                        and `qualified_name`.
    /// - `include_traits`   — Trait/Interface nodes glob-matched on `name`
    ///                        and `qualified_name`.
    /// - `include_modules`  — Module/Package nodes glob-matched on
    ///                        `qualified_name` (e.g. `*::merge*`).
    /// - `include_endpoints`— Endpoint nodes glob-matched on the route path
    ///                        (`name` then `qualified_name`).
    /// - `include_specs`    — nodes whose `spec_path`/`spec_paths` (or a
    ///                        `GovernedBy` edge to a spec node) references a
    ///                        listed spec; the spec nodes themselves are
    ///                        included too.
    ///
    /// Edges are included when both endpoints are in the matched node set.
    /// Soft-deleted nodes/edges (`deleted_at` set) are never included.
    pub fn project<'a>(
        &self,
        nodes: impl IntoIterator<Item = &'a GraphNode>,
        edges: impl IntoIterator<Item = &'a GraphEdge>,
    ) -> (Vec<GraphNode>, Vec<GraphEdge>) {
        let all_nodes: Vec<&GraphNode> = nodes
            .into_iter()
            .filter(|n| n.deleted_at.is_none())
            .collect();
        let all_edges: Vec<&GraphEdge> = edges
            .into_iter()
            .filter(|e| e.deleted_at.is_none())
            .collect();

        // Pre-resolve spec linkage for `include_specs`:
        // - the spec node itself (name/qualified_name is the spec path), and
        // - every node governed by a listed spec (GovernedBy source), plus the
        //   GovernedBy target spec node.
        let node_index: std::collections::HashMap<&str, &GraphNode> = all_nodes
            .iter()
            .map(|n| (n.id.as_str(), *n))
            .collect();
        let mut spec_linked_ids: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for e in all_edges.iter().filter(|e| e.edge_type == EdgeType::GovernedBy) {
            if let Some(target) = node_index.get(e.target_id.as_str()) {
                if self.spec_node_matches(target) {
                    spec_linked_ids.insert(e.target_id.as_str());
                    spec_linked_ids.insert(e.source_id.as_str());
                }
            }
        }
        // Endpoint route paths (`/api/v1/auth/login`) live in RoutesTo/Contains
        // edge metadata as `{"path": "..."}`; index them by endpoint node id.
        let endpoint_routes: std::collections::HashMap<&str, String> = all_edges
            .iter()
            .filter_map(|e| {
                let path = e.metadata.as_deref().and_then(edge_route_path)?;
                Some((e.source_id.as_str(), path))
            })
            .collect();

        let matched: Vec<GraphNode> = all_nodes
            .into_iter()
            .filter(|n| {
                self.matches_node(n, &spec_linked_ids, &endpoint_routes)
            })
            .cloned()
            .collect();

        let matched_ids: std::collections::HashSet<&str> =
            matched.iter().map(|n| n.id.as_str()).collect();

        let matched_edges: Vec<GraphEdge> = all_edges
            .into_iter()
            .filter(|e| {
                matched_ids.contains(e.source_id.as_str())
                    && matched_ids.contains(e.target_id.as_str())
            })
            .cloned()
            .collect();

        (matched, matched_edges)
    }

    /// Does a single node match any include rule of this concept view?
    fn matches_node(
        &self,
        n: &GraphNode,
        governed_spec_node_ids: &std::collections::HashSet<&str>,
        endpoint_routes: &std::collections::HashMap<&str, String>,
    ) -> bool {
        match n.node_type {
            NodeType::Type | NodeType::Class | NodeType::Enum => {
                glob_any(&self.include_types, &n.name)
                    || glob_any(&self.include_types, &n.qualified_name)
                    || self.matches_spec_include(n, governed_spec_node_ids)
            }
            NodeType::Trait | NodeType::Interface => {
                glob_any(&self.include_traits, &n.name)
                    || glob_any(&self.include_traits, &n.qualified_name)
                    || self.matches_spec_include(n, governed_spec_node_ids)
            }
            NodeType::Module | NodeType::Package => {
                glob_any(&self.include_modules, &n.qualified_name)
                    || glob_any(&self.include_modules, &n.name)
                    || self.matches_spec_include(n, governed_spec_node_ids)
            }
            NodeType::Endpoint => {
                // The route path (`/api/v1/auth/login`) is the canonical
                // match target (RoutesTo/Contains edge metadata), with the
                // node names as fallback.
                endpoint_routes
                    .get(n.id.as_str())
                    .is_some_and(|route| glob_any(&self.include_endpoints, route))
                    || glob_any(&self.include_endpoints, &n.name)
                    || glob_any(&self.include_endpoints, &n.qualified_name)
                    || self.matches_spec_include(n, governed_spec_node_ids)
            }
            _ => self.matches_spec_include(n, governed_spec_node_ids),
        }
    }

    /// `include_specs` rule: the node's own spec linkage (spec_path /
    /// spec_paths), a GovernedBy edge to a listed spec, or the node being a
    /// spec node referenced by the concept.
    fn matches_spec_include(
        &self,
        n: &GraphNode,
        spec_linked_ids: &std::collections::HashSet<&str>,
    ) -> bool {
        if self.include_specs.is_empty() {
            return false;
        }
        let path_matches = |sp: &str| {
            glob_any(&self.include_specs, sp) || glob_any(&self.include_specs, spec_file_name(sp))
        };
        // Nodes whose recorded spec linkage references a listed spec.
        if n.spec_path.as_deref().is_some_and(path_matches)
            || n.spec_paths.iter().any(|sp| path_matches(sp))
        {
            return true;
        }
        // Extractor-created spec nodes carry the spec path as name/qualified_name.
        if path_matches(&n.name) || path_matches(&n.qualified_name) {
            return true;
        }
        // The node or its GovernedBy-reachable spec is linked to a listed spec.
        spec_linked_ids.contains(n.id.as_str())
    }

    /// Does this node look like a spec node (extractor convention: name or
    /// qualified_name is the spec path) matching a listed spec?
    fn spec_node_matches(&self, n: &GraphNode) -> bool {
        if self.include_specs.is_empty() {
            return false;
        }
        let path_matches = |sp: &str| {
            glob_any(&self.include_specs, sp) || glob_any(&self.include_specs, spec_file_name(sp))
        };
        path_matches(&n.name) || path_matches(&n.qualified_name)
    }
}

#[cfg(test)]
mod concept_tests {
    use super::*;

    fn node(repo: &str, name: &str, node_type: NodeType, qname: &str) -> GraphNode {
        GraphNode {
            id: Id::new(format!("id-{name}")),
            repo_id: Id::new(repo),
            node_type,
            name: name.to_string(),
            qualified_name: qname.to_string(),
            file_path: format!("src/{name}.rs"),
            line_start: 1,
            line_end: 2,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc".to_string(),
            last_modified_by: None,
            last_modified_at: 0,
            created_sha: "abc".to_string(),
            created_at: 0,
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

    fn edge(repo: &str, id: &str, src: &str, tgt: &str, edge_type: EdgeType) -> GraphEdge {
        GraphEdge {
            id: Id::new(id),
            repo_id: Id::new(repo),
            source_id: Id::new(src),
            target_id: Id::new(tgt),
            edge_type,
            metadata: None,
            first_seen_at: 0,
            last_seen_at: 0,
            deleted_at: None,
        }
    }

    #[test]
    fn glob_match_anchoring() {
        // `Gate*` matches GateApprovals but not LateGate.
        assert!(glob_match("Gate*", "GateApprovals"));
        assert!(!glob_match("Gate*", "LateGate"));
        // `*::merge*` matches qualified module names.
        assert!(glob_match("*::merge*", "gyre_domain::merge::queue"));
        assert!(!glob_match("*::merge*", "gyre_domain::marker"));
        // Exact pattern matches exactly — no implicit wildcards.
        assert!(glob_match("MergeRequest", "MergeRequest"));
        assert!(!glob_match("MergeRequest", "MergeRequestDependency"));
        assert!(!glob_match("MergeRequest", "AMergeRequest"));
        // Infix wildcard.
        assert!(glob_match("*Auth*", "JwtAuthValidator"));
        assert!(!glob_match("*Auth*", "JwtToken"));
        // Endpoint prefix wildcard.
        assert!(glob_match("/api/v1/auth/*", "/api/v1/auth/login"));
        assert!(!glob_match("/api/v1/auth/*", "/api/v1/users/list"));
        // Bare `*` matches anything; empty pattern only empty text.
        assert!(glob_match("*", "anything"));
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
        // Case-sensitive.
        assert!(!glob_match("*auth*", "JwtAuthValidator"));
    }

    #[test]
    fn concept_projection_unions_all_include_rules() {
        let concept = ConceptView {
            name: "Authentication".to_string(),
            description: String::new(),
            include_types: vec!["*Auth*".to_string()],
            include_traits: vec!["*Auth*".to_string()],
            include_modules: vec!["*::auth*".to_string()],
            include_endpoints: vec!["/api/v1/auth/*".to_string()],
            include_specs: vec!["identity-security.md".to_string()],
        };

        let auth_type = node("r", "JwtAuthValidator", NodeType::Type, "crates::JwtAuthValidator");
        let plain_type = node("r", "Invoice", NodeType::Type, "crates::Invoice");
        let auth_trait = node("r", "AuthProvider", NodeType::Trait, "crates::AuthProvider");
        let auth_module = node("r", "auth", NodeType::Module, "gyre_domain::auth::tokens");
        let other_module = node("r", "billing", NodeType::Module, "gyre_domain::billing");
        let auth_endpoint = node("r", "login", NodeType::Endpoint, "api::login");
        let governed = node("r", "TokenStore", NodeType::Type, "crates::TokenStore");
        let mut governed = governed;
        governed.spec_path = Some("specs/system/identity-security.md".to_string());

        let nodes = vec![
            auth_type.clone(),
            plain_type.clone(),
            auth_trait.clone(),
            auth_module.clone(),
            other_module.clone(),
            auth_endpoint.clone(),
            governed.clone(),
        ];
        // RoutesTo edge carries the route path in metadata (extractor convention).
        let routes = edge(
            "r",
            "e0",
            &auth_endpoint.id.to_string(),
            &auth_type.id.to_string(),
            EdgeType::RoutesTo,
        );
        let mut routes = routes;
        routes.metadata = Some(r#"{"path":"/api/v1/auth/login","method":"POST"}"#.to_string());
        let edges = vec![
            routes,
            edge("r", "e1", &auth_type.id.to_string(), &governed.id.to_string(), EdgeType::DependsOn),
            edge("r", "e2", &plain_type.id.to_string(), &governed.id.to_string(), EdgeType::DependsOn),
        ];

        let (matched_nodes, matched_edges) = concept.project(nodes.iter(), edges.iter());

        let names: Vec<&str> = matched_nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"JwtAuthValidator"), "type glob must match");
        assert!(names.contains(&"AuthProvider"), "trait glob must match");
        assert!(names.contains(&"auth"), "module glob must match");
        assert!(!names.contains(&"billing"), "unrelated module must not match");
        assert!(names.contains(&"login"), "endpoint glob must match");
        assert!(names.contains(&"TokenStore"), "spec-governed node must match");
        assert!(!names.contains(&"Invoice"), "unrelated type must not match");

        // e0 (endpoint→type) and e1 (type→governed) have both endpoints
        // matched; e2 has one unmatched endpoint.
        let mut matched_edge_ids: Vec<&str> =
            matched_edges.iter().map(|e| e.id.as_str()).collect();
        matched_edge_ids.sort_unstable();
        assert_eq!(matched_edge_ids, vec!["e0", "e1"]);
    }

    #[test]
    fn concept_projection_governed_by_edge_matches() {
        let concept = ConceptView {
            name: "C".to_string(),
            description: String::new(),
            include_types: vec![],
            include_traits: vec![],
            include_modules: vec![],
            include_endpoints: vec![],
            include_specs: vec!["abac-policy-engine.md".to_string()],
        };
        // Spec node named by path (extractor convention: Module node whose
        // name/qualified_name is the spec path).
        let spec_node = node("r", "specs/system/abac-policy-engine.md", NodeType::Module, "specs/system/abac-policy-engine.md");
        let code = node("r", "PolicyEngine", NodeType::Type, "crates::PolicyEngine");
        let nodes = vec![spec_node.clone(), code.clone()];
        let edges = vec![edge(
            "r",
            "e1",
            &code.id.to_string(),
            &spec_node.id.to_string(),
            EdgeType::GovernedBy,
        )];

        let (matched, _) = concept.project(nodes.iter(), edges.iter());
        // The code node is the GovernedBy source; the spec node is the target.
        let ids: Vec<&str> = matched.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&spec_node.id.as_str()), "spec node must be included");
        assert!(
            ids.contains(&code.id.as_str()),
            "node governed by listed spec must be included"
        );
    }

    #[test]
    fn concept_projection_excludes_deleted() {
        let concept = ConceptView {
            name: "C".to_string(),
            description: String::new(),
            include_types: vec!["*".to_string()],
            include_traits: vec![],
            include_modules: vec![],
            include_endpoints: vec![],
            include_specs: vec![],
        };
        let mut deleted = node("r", "DeletedType", NodeType::Type, "crates::DeletedType");
        deleted.deleted_at = Some(1);
        let active = node("r", "ActiveType", NodeType::Type, "crates::ActiveType");
        let nodes = vec![deleted, active];
        let (matched, _) = concept.project(nodes.iter(), std::iter::empty());
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "ActiveType");
    }
}
