//! View Query Resolver — resolves view queries against the knowledge graph.
//!
//! This is the core engine that both the dry-run MCP tool and the frontend renderer use.
//! Given a ViewQuery and a set of nodes/edges, produces a resolved result set with
//! matched nodes, groups, callouts, narrative steps, and warnings.

use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence};
use gyre_common::view_query::{Scope, ViewQuery};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

// ── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedGroup {
    pub name: String,
    pub matched: usize,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCallout {
    pub node: String,
    pub text: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedNarrative {
    pub node: String,
    pub text: String,
    pub order: Option<u32>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub matched_nodes: usize,
    pub matched_node_names: Vec<String>,
    pub groups_resolved: Vec<ResolvedGroup>,
    pub callouts_resolved: usize,
    pub callouts_unresolved: Vec<String>,
    pub narrative_resolved: usize,
    pub warnings: Vec<String>,
    /// Per-node BFS depth from focus node (for tiered_colors emphasis).
    /// Only populated when scope is Focus or Concept.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub node_depths: HashMap<String, u32>,
    /// Per-node metric values (for heat emphasis).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub node_metrics: HashMap<String, f64>,
    /// Edges filtered to connections between matched nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_edges: Vec<MatchedEdge>,
    /// Resolved annotation with template variables replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_annotation: Option<ResolvedAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAnnotation {
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedEdge {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummary {
    pub repo_id: String,
    pub node_counts: HashMap<String, usize>,
    pub edge_counts: HashMap<String, usize>,
    pub top_types_by_fields: Vec<String>,
    pub top_functions_by_calls: Vec<String>,
    pub modules: Vec<String>,
    pub test_coverage: TestCoverageSummary,
    /// Spec coverage: how many nodes have a governing spec
    #[serde(default)]
    pub spec_coverage: SpecCoverageSummary,
    /// Percentage of nodes with spec_path or GovernedBy edges (0.0-100.0)
    #[serde(default)]
    pub spec_coverage_pct: f64,
    /// Percentage of testable nodes (functions/methods) reachable from test nodes (0.0-100.0)
    #[serde(default)]
    pub test_coverage_pct: f64,
    /// Risk indicators: high-complexity untested code, unspecced hot paths,
    /// and nodes with complexity > p90 and low coverage
    #[serde(default)]
    pub risk_indicators: Vec<String>,
    /// Anomaly summary: quick counts of high-complexity nodes, orphan functions, untested nodes
    #[serde(default)]
    pub anomaly_summary: AnomalySummary,
    /// Count of nodes modified in the last 7 days (based on last_modified_at)
    #[serde(default)]
    pub recent_changes_7d: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnomalySummary {
    /// Functions with complexity > 20
    pub high_complexity_count: usize,
    /// Functions with no incoming calls and no outgoing calls (orphans)
    pub orphan_function_count: usize,
    /// Functions not reachable from any test node
    pub untested_function_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpecCoverageSummary {
    pub governed: usize,
    pub unspecced: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCoverageSummary {
    pub test_functions: usize,
    pub reachable_from_tests: usize,
    pub unreachable: usize,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parse_edge_type(s: &str) -> Option<EdgeType> {
    match s.to_lowercase().as_str() {
        "calls" => Some(EdgeType::Calls),
        "contains" => Some(EdgeType::Contains),
        "implements" => Some(EdgeType::Implements),
        "depends_on" | "dependson" => Some(EdgeType::DependsOn),
        "field_of" | "fieldof" => Some(EdgeType::FieldOf),
        "returns" => Some(EdgeType::Returns),
        "routes_to" | "routesto" => Some(EdgeType::RoutesTo),
        "renders" => Some(EdgeType::Renders),
        "persists_to" | "persiststo" => Some(EdgeType::PersistsTo),
        "governed_by" | "governedby" => Some(EdgeType::GovernedBy),
        "produced_by" | "producedby" => Some(EdgeType::ProducedBy),
        _ => None,
    }
}

fn parse_node_type(s: &str) -> Option<NodeType> {
    match s.to_lowercase().as_str() {
        "package" => Some(NodeType::Package),
        "module" => Some(NodeType::Module),
        "type" | "struct" => Some(NodeType::Type),
        "trait" => Some(NodeType::Trait),
        "interface" => Some(NodeType::Interface),
        "function" => Some(NodeType::Function),
        "method" => Some(NodeType::Method),
        "class" => Some(NodeType::Class),
        "enum" => Some(NodeType::Enum),
        "enum_variant" | "enumvariant" | "variant" => Some(NodeType::EnumVariant),
        "endpoint" => Some(NodeType::Endpoint),
        "component" => Some(NodeType::Component),
        "table" => Some(NodeType::Table),
        "constant" => Some(NodeType::Constant),
        "field" => Some(NodeType::Field),
        "spec" => Some(NodeType::Spec),
        _ => None,
    }
}

/// Public wrapper for node_type_str (used by server for LLM prompt).
pub fn node_type_str_pub(nt: &NodeType) -> &'static str {
    node_type_str(nt)
}

/// Public wrapper for edge_type_str (used by server for LLM prompt).
pub fn edge_type_str_pub(et: &EdgeType) -> &'static str {
    edge_type_str(et)
}

fn node_type_str(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Package => "package",
        NodeType::Module => "module",
        NodeType::Type => "type",
        NodeType::Trait => "trait",
        NodeType::Interface => "interface",
        NodeType::Function => "function",
        NodeType::Method => "method",
        NodeType::Class => "class",
        NodeType::Enum => "enum",
        NodeType::EnumVariant => "enum_variant",
        NodeType::Endpoint => "endpoint",
        NodeType::Component => "component",
        NodeType::Table => "table",
        NodeType::Constant => "constant",
        NodeType::Field => "field",
        NodeType::Spec => "spec",
    }
}

fn edge_type_str(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::Contains => "contains",
        EdgeType::Implements => "implements",
        EdgeType::DependsOn => "depends_on",
        EdgeType::Calls => "calls",
        EdgeType::FieldOf => "field_of",
        EdgeType::Returns => "returns",
        EdgeType::RoutesTo => "routes_to",
        EdgeType::Renders => "renders",
        EdgeType::PersistsTo => "persists_to",
        EdgeType::GovernedBy => "governed_by",
        EdgeType::ProducedBy => "produced_by",
    }
}

/// Build adjacency maps for the graph (both directions).
fn build_adjacency(
    edges: &[GraphEdge],
) -> (
    HashMap<String, Vec<(String, EdgeType)>>,
    HashMap<String, Vec<(String, EdgeType)>>,
) {
    let mut outgoing: HashMap<String, Vec<(String, EdgeType)>> = HashMap::new();
    let mut incoming: HashMap<String, Vec<(String, EdgeType)>> = HashMap::new();

    for edge in edges {
        if edge.deleted_at.is_some() {
            continue;
        }
        outgoing
            .entry(edge.source_id.to_string())
            .or_default()
            .push((edge.target_id.to_string(), edge.edge_type.clone()));
        incoming
            .entry(edge.target_id.to_string())
            .or_default()
            .push((edge.source_id.to_string(), edge.edge_type.clone()));
    }

    (outgoing, incoming)
}

/// BFS traversal from a starting node along specified edge types and direction.
fn bfs_traverse(
    start_id: &str,
    edge_types: &[EdgeType],
    direction: &str,
    depth: u32,
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashSet<String> {
    bfs_traverse_with_depths(start_id, edge_types, direction, depth, outgoing, incoming)
        .into_keys()
        .collect()
}

/// BFS traversal that returns a map of node_id → BFS depth.
fn bfs_traverse_with_depths(
    start_id: &str,
    edge_types: &[EdgeType],
    direction: &str,
    depth: u32,
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashMap<String, u32> {
    let mut visited: HashMap<String, u32> = HashMap::new();
    let mut queue = VecDeque::new();
    visited.insert(start_id.to_string(), 0);
    queue.push_back((start_id.to_string(), 0u32));

    while let Some((current, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }

        let neighbors: Vec<&(String, EdgeType)> = match direction {
            "incoming" => incoming
                .get(&current)
                .map(|v| v.iter().collect())
                .unwrap_or_default(),
            "both" => {
                let mut all: Vec<&(String, EdgeType)> = Vec::new();
                if let Some(out) = outgoing.get(&current) {
                    all.extend(out.iter());
                }
                if let Some(inc) = incoming.get(&current) {
                    all.extend(inc.iter());
                }
                all
            }
            _ => outgoing
                .get(&current)
                .map(|v| v.iter().collect())
                .unwrap_or_default(),
        };

        for (neighbor_id, et) in neighbors {
            if edge_types.is_empty() || edge_types.contains(et) {
                if !visited.contains_key(neighbor_id) {
                    visited.insert(neighbor_id.clone(), d + 1);
                    queue.push_back((neighbor_id.clone(), d + 1));
                }
            }
        }
    }

    visited
}

/// Find a node by name or qualified_name (case-insensitive).
/// Deterministic: exact match first, then shortest partial match with alphabetical tiebreak.
fn find_node_by_name<'a>(nodes: &[&'a GraphNode], name: &str) -> Option<&'a GraphNode> {
    find_node_by_name_with_match_type(nodes, name).map(|(n, _)| n)
}

/// Find a node by name, returning whether the match was partial (not exact).
/// Deterministic priority: exact match > case-sensitive prefix > case-insensitive prefix >
/// case-sensitive substring > case-insensitive substring.
/// When multiple nodes match at the same level, prefer the shorter qualified_name.
fn find_node_by_name_with_match_type<'a>(
    nodes: &[&'a GraphNode],
    name: &str,
) -> Option<(&'a GraphNode, bool)> {
    let lower = name.to_lowercase();

    // 1. Exact match (case-insensitive) on qualified_name or name.
    if let Some(exact) = nodes
        .iter()
        .find(|n| n.qualified_name.to_lowercase() == lower || n.name.to_lowercase() == lower)
    {
        return Some((exact, false));
    }

    // Helper: pick the candidate with the shortest qualified_name for determinism.
    let pick_best = |candidates: &mut Vec<&'a GraphNode>| -> Option<&'a GraphNode> {
        candidates.sort_by(|a, b| {
            a.qualified_name
                .len()
                .cmp(&b.qualified_name.len())
                .then_with(|| a.qualified_name.cmp(&b.qualified_name))
        });
        candidates.first().copied()
    };

    // 2. Case-sensitive prefix match on name or qualified_name.
    let mut cs_prefix: Vec<&'a GraphNode> = nodes
        .iter()
        .filter(|n| n.name.starts_with(name) || n.qualified_name.starts_with(name))
        .copied()
        .collect();
    if let Some(found) = pick_best(&mut cs_prefix) {
        return Some((found, true));
    }

    // 3. Case-insensitive prefix match on name or qualified_name.
    let mut ci_prefix: Vec<&'a GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.name.to_lowercase().starts_with(&lower)
                || n.qualified_name.to_lowercase().starts_with(&lower)
        })
        .copied()
        .collect();
    if let Some(found) = pick_best(&mut ci_prefix) {
        return Some((found, true));
    }

    // 4. Case-sensitive substring match on name or qualified_name.
    let mut cs_substr: Vec<&'a GraphNode> = nodes
        .iter()
        .filter(|n| n.name.contains(name) || n.qualified_name.contains(name))
        .copied()
        .collect();
    if let Some(found) = pick_best(&mut cs_substr) {
        return Some((found, true));
    }

    // 5. Case-insensitive substring match on name or qualified_name.
    let mut ci_substr: Vec<&'a GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.name.to_lowercase().contains(&lower)
                || n.qualified_name.to_lowercase().contains(&lower)
        })
        .copied()
        .collect();
    if let Some(found) = pick_best(&mut ci_substr) {
        return Some((found, true));
    }

    None
}

/// Edge types traversed for test reachability analysis.
/// Tests can reach code via direct calls, trait dispatch (Implements),
/// and HTTP endpoint tests (RoutesTo).
/// NOTE: Contains is intentionally excluded — including it would make all
/// sibling functions in a module "reachable" just because one test exists
/// in the same module, inflating test coverage metrics.
const TEST_REACHABILITY_EDGES: &[EdgeType] =
    &[EdgeType::Calls, EdgeType::Implements, EdgeType::RoutesTo];

/// Compute the set of nodes reachable from test functions.
/// Pre-computes a single BFS from all test nodes for O(T + N + M) total.
/// Accepts `&[&GraphNode]` to avoid cloning node slices.
fn compute_test_reachable(
    nodes: &[&GraphNode],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashSet<String> {
    let test_node_ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.test_node && n.deleted_at.is_none())
        .map(|n| n.id.to_string())
        .collect();

    // Multi-source BFS: start from ALL test nodes simultaneously.
    // This is O(N + M) instead of O(T * (N + M)).
    let mut reachable = HashSet::new();
    let mut queue: std::collections::VecDeque<(String, u32)> =
        test_node_ids.iter().map(|id| (id.clone(), 0)).collect();
    while let Some((current, depth)) = queue.pop_front() {
        if depth > 100 {
            continue;
        }
        if !reachable.insert(current.clone()) {
            continue;
        }
        if let Some(neighbors) = outgoing.get(&current) {
            for (next, et) in neighbors {
                if TEST_REACHABILITY_EDGES.contains(et) && !reachable.contains(next) {
                    queue.push_back((next.clone(), depth + 1));
                }
            }
        }
    }
    reachable
}

/// Compute the number of distinct tests that can reach a node (test fragility).
/// Uses a single multi-source BFS per test node with depth limit, then counts
/// how many test BFS trees contain the target node.
/// Accepts `&[&GraphNode]` to avoid cloning node slices.
#[allow(dead_code)] // Kept for potential use in per-node metric queries
fn compute_test_fragility_count(
    node_id: &str,
    nodes: &[&GraphNode],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> usize {
    let test_ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.test_node && n.deleted_at.is_none())
        .map(|n| n.id.to_string())
        .collect();
    let mut count = 0;
    for tid in &test_ids {
        // Depth-limited BFS per test (20 hops is sufficient for fragility)
        let reached = bfs_traverse(
            tid,
            TEST_REACHABILITY_EDGES,
            "outgoing",
            20,
            outgoing,
            incoming,
        );
        if reached.contains(node_id) {
            count += 1;
        }
    }
    count
}

/// Pre-compute test fragility counts for ALL nodes at once.
/// Returns a map from node_id → count of distinct tests that reach it.
/// This is O(T * (N + M)) total instead of O(T * (N + M)) per node queried.
fn compute_all_test_fragility(
    nodes: &[&GraphNode],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashMap<String, usize> {
    let test_ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.test_node && n.deleted_at.is_none())
        .map(|n| n.id.to_string())
        .collect();
    let mut fragility: HashMap<String, usize> = HashMap::new();
    for tid in &test_ids {
        let reached = bfs_traverse(
            tid,
            TEST_REACHABILITY_EDGES,
            "outgoing",
            20,
            outgoing,
            incoming,
        );
        for nid in reached {
            *fragility.entry(nid).or_default() += 1;
        }
    }
    fragility
}

/// Count incoming Calls edges to a node.
fn count_incoming_calls(
    node_id: &str,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> usize {
    incoming
        .get(node_id)
        .map(|neighbors| {
            neighbors
                .iter()
                .filter(|(_, et)| *et == EdgeType::Calls)
                .count()
        })
        .unwrap_or(0)
}

/// Count outgoing Calls edges from a node.
fn count_outgoing_calls(
    node_id: &str,
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
) -> usize {
    outgoing
        .get(node_id)
        .map(|neighbors| {
            neighbors
                .iter()
                .filter(|(_, et)| *et == EdgeType::Calls)
                .count()
        })
        .unwrap_or(0)
}

/// Count FieldOf edges incoming to a node (field count).
fn count_fields(node_id: &str, incoming: &HashMap<String, Vec<(String, EdgeType)>>) -> usize {
    incoming
        .get(node_id)
        .map(|neighbors| {
            neighbors
                .iter()
                .filter(|(_, et)| *et == EdgeType::FieldOf)
                .count()
        })
        .unwrap_or(0)
}

// ── Core resolver ────────────────────────────────────────────────────────────

/// Result of scope resolution including optional depth information.
pub struct ScopeResult {
    pub matched: HashSet<String>,
    pub depths: HashMap<String, u32>,
    /// Warnings generated during scope resolution.
    pub warnings: Vec<String>,
}

/// Resolve a view query scope against the graph, returning the set of matched node IDs.
pub fn resolve_scope(
    scope: &Scope,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
) -> HashSet<String> {
    resolve_scope_with_depths(scope, nodes, edges, selected_node_id).matched
}

/// Resolve a scope and also return per-node BFS depths (for tiered_colors).
pub fn resolve_scope_with_depths(
    scope: &Scope,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
) -> ScopeResult {
    let (outgoing, incoming) = build_adjacency(edges);
    resolve_scope_with_adjacency(scope, nodes, edges, selected_node_id, &outgoing, &incoming)
}

/// Resolve a scope using pre-built adjacency maps (avoids rebuilding them).
fn resolve_scope_with_adjacency(
    scope: &Scope,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> ScopeResult {
    let active_nodes: Vec<&GraphNode> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
    let all_ids: HashSet<String> = active_nodes.iter().map(|n| n.id.to_string()).collect();

    let no_depths = || HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    let (matched, depths) = match scope {
        Scope::All => (all_ids, no_depths()),

        Scope::Focus {
            node,
            edges: edge_strs,
            direction,
            depth,
        } => {
            let resolved_name = if node == "$clicked" || node == "$selected" {
                selected_node_id.unwrap_or("")
            } else {
                node.as_str()
            };

            let start_node = if resolved_name.is_empty() {
                None
            } else {
                active_nodes
                    .iter()
                    .find(|n| n.id.to_string() == resolved_name)
                    .copied()
                    .or_else(|| find_node_by_name(&active_nodes, resolved_name))
            };

            match start_node {
                Some(sn) => {
                    let edge_types: Vec<EdgeType> = edge_strs
                        .iter()
                        .filter_map(|s| parse_edge_type(s))
                        .collect();
                    let depth_map = bfs_traverse_with_depths(
                        &sn.id.to_string(),
                        &edge_types,
                        direction,
                        *depth,
                        outgoing,
                        incoming,
                    );
                    let ids = depth_map.keys().cloned().collect();
                    (ids, depth_map)
                }
                None => {
                    if !resolved_name.is_empty() {
                        warnings.push(format!(
                            "Focus node '{}' not found in the graph. Check the name or use graph_nodes to find the correct qualified_name.",
                            resolved_name
                        ));
                    }
                    (HashSet::new(), no_depths())
                }
            }
        }

        Scope::Filter {
            node_types,
            computed,
            name_pattern,
        } => {
            // Parse node_types and name_pattern first — they apply as additional
            // filters even when computed is set (spec: Filter has BOTH).
            for nt_str in node_types {
                if parse_node_type(nt_str).is_none() {
                    warnings.push(format!(
                        "Unrecognized node type '{}' in filter — ignored. Valid types: package, module, type, interface, function, endpoint, component, table, constant, field, spec",
                        nt_str
                    ));
                }
            }
            let types: Vec<NodeType> = node_types
                .iter()
                .filter_map(|s| parse_node_type(s))
                .collect();
            let pattern = name_pattern.as_ref().map(|p| p.to_lowercase());

            let base_set = if let Some(expr) = computed {
                // Validate expression before resolving — catches typos like $caller vs $callers
                if let Some(validation_err) = validate_computed_expression(expr) {
                    warnings.push(format!(
                        "Computed expression '{}' is invalid: {}",
                        expr, validation_err
                    ));
                }
                let result = resolve_computed_expression(
                    expr,
                    &active_nodes,
                    edges,
                    outgoing,
                    incoming,
                    selected_node_id,
                );
                if result.is_empty() {
                    // Detect trace-based metrics that always return empty in static analysis
                    let trace_metrics = ["span_duration", "span_count", "error_rate"];
                    let is_trace_query = trace_metrics.iter().any(|m| expr.contains(m));
                    if is_trace_query {
                        warnings.push(format!(
                            "[info] Computed expression '{}' matched 0 nodes — trace-based metrics \
                             (span_duration, span_count, error_rate) require OTLP runtime data and \
                             are not available in static analysis. Use structural metrics instead: \
                             complexity, churn, incoming_calls, test_coverage, risk_score.",
                            expr
                        ));
                    } else {
                        warnings.push(format!("Computed expression '{}' matched 0 nodes", expr));
                    }
                }
                result
            } else {
                // No computed expression — start from all active nodes
                active_nodes.iter().map(|n| n.id.to_string()).collect()
            };

            // Apply node_types and name_pattern as additional filters on the base set
            let result = if types.is_empty() && pattern.is_none() {
                base_set
            } else {
                let node_map: std::collections::HashMap<String, &GraphNode> = active_nodes
                    .iter()
                    .map(|n| (n.id.to_string(), *n))
                    .collect();
                base_set
                    .into_iter()
                    .filter(|id| {
                        if let Some(n) = node_map.get(id) {
                            let type_match = types.is_empty() || types.contains(&n.node_type);
                            let name_match = pattern.as_ref().map_or(true, |p| {
                                n.name.to_lowercase().contains(p)
                                    || n.qualified_name.to_lowercase().contains(p)
                            });
                            type_match && name_match
                        } else {
                            false
                        }
                    })
                    .collect()
            };
            (result, no_depths())
        }

        Scope::TestGaps => {
            let reachable = compute_test_reachable(&active_nodes, outgoing);
            // Include Functions, Endpoints, and Types — not just Functions.
            // Endpoints and Types without test coverage are high-risk gaps.
            let result = active_nodes
                .iter()
                .filter(|n| !reachable.contains(&n.id.to_string()))
                .filter(|n| {
                    matches!(
                        n.node_type,
                        NodeType::Function
                            | NodeType::Method
                            | NodeType::Endpoint
                            | NodeType::Type
                            | NodeType::Trait
                            | NodeType::Class
                    )
                })
                .map(|n| n.id.to_string())
                .collect();
            (result, no_depths())
        }

        Scope::Diff {
            from_commit,
            to_commit,
        } => {
            // Warn if from and to are identical
            if from_commit == to_commit {
                warnings.push(format!(
                    "Diff from '{}' to '{}' refers to the same commit; the diff will be empty.",
                    from_commit, to_commit
                ));
            }
            // Find nodes that changed between two commits by looking at SHA matches.
            // A node is "changed" if its created_sha or last_modified_sha matches ANY
            // commit in the range. Since we don't have git history, we use a best-effort
            // approach: include nodes whose last_modified_sha or created_sha matches
            // the to_commit (modified in the target), AND exclude nodes that were
            // created before the from_commit by checking created_sha.
            //
            // Also supports timestamp-based diff when commit SHAs contain "~" prefix
            // (e.g., "~1712000000" for epoch seconds) for temporal queries.
            let from_lower = from_commit.to_lowercase();
            let to_lower = to_commit.to_lowercase();

            // If from_commit starts with "~", interpret as epoch timestamp range
            let from_ts = from_lower
                .strip_prefix('~')
                .and_then(|s| s.parse::<u64>().ok());
            let to_ts = to_lower
                .strip_prefix('~')
                .and_then(|s| s.parse::<u64>().ok());

            let result = if let (Some(from_epoch), Some(to_epoch)) = (from_ts, to_ts) {
                // Temporal diff: nodes created or modified between the two timestamps
                active_nodes
                    .iter()
                    .filter(|n| {
                        let modified = n.last_modified_at;
                        let created = n.created_at;
                        // Node was created or modified within the time range
                        (created > from_epoch && created <= to_epoch)
                            || (modified > from_epoch && modified <= to_epoch)
                    })
                    .map(|n| n.id.to_string())
                    .collect()
            } else {
                // SHA-based diff: best-effort heuristic for finding nodes changed
                // between two commits.
                //
                // Limitations (inherent to the graph model):
                //   - Each node stores only `created_sha` and `last_modified_sha`.
                //     We have no access to the full git log, so nodes changed by
                //     intermediate commits between from_commit and to_commit will
                //     be missed unless their last_modified_sha still points to one
                //     of the boundary commits.
                //   - Prefix matching requires at least 7 hex characters to avoid
                //     false positives on short SHA fragments.
                //
                // For accurate range queries, prefer temporal diff with epoch
                // prefixes (e.g., Scope::Diff { from: "~1712000000", to: "~1712100000" }).
                warnings.push(
                    "[info] SHA-based diff shows an approximate result. Each node only \
                     records the commit that created it and the commit that last modified it, \
                     so changes made by commits between the two boundary commits may not \
                     appear. For a complete range, use a temporal diff with epoch timestamps \
                     (e.g., Scope::Diff { from: \"~1712000000\", to: \"~1712100000\" })."
                        .to_string(),
                );
                let sha_matches = |sha: &str, target: &str| -> bool {
                    if target.is_empty() || target.len() < 7 {
                        return false; // Require at least 7 hex chars to prevent false positives
                    }
                    let sha_lower = sha.to_lowercase();
                    // Proper prefix matching: the query target must be a prefix of the
                    // node's SHA. Do NOT match the reverse direction (node SHA as prefix
                    // of query) as that causes false positives with abbreviated node data.
                    sha_lower.starts_with(target) || sha_lower == target
                };

                active_nodes
                    .iter()
                    .filter(|n| {
                        let created_matches_to = sha_matches(&n.created_sha, &to_lower);
                        let modified_matches_to = sha_matches(&n.last_modified_sha, &to_lower);
                        let created_matches_from = sha_matches(&n.created_sha, &from_lower);
                        let modified_matches_from = sha_matches(&n.last_modified_sha, &from_lower);

                        // Include nodes created or modified at to_commit.
                        // Exclude nodes that already existed unchanged at from_commit
                        // (created AND last-modified at or before from_commit).
                        (created_matches_to || modified_matches_to)
                            && !(created_matches_from && modified_matches_from)
                    })
                    .map(|n| n.id.to_string())
                    .collect()
            };
            (result, no_depths())
        }

        Scope::Concept {
            seed_nodes,
            expand_edges,
            expand_depth,
            expand_direction,
        } => {
            let edge_types: Vec<EdgeType> = expand_edges
                .iter()
                .filter_map(|s| parse_edge_type(s))
                .collect();
            // Default to "both" (consistent with the ViewQuery struct's default).
            // Empty string is treated as default rather than a distinct value.
            let direction = if expand_direction.is_empty() {
                "both"
            } else {
                expand_direction.as_str()
            };
            let mut result = HashSet::new();
            let mut all_depths = HashMap::new();

            let total_seeds = seed_nodes.len();
            let mut resolved_seeds = 0usize;
            for seed_name in seed_nodes {
                if let Some(seed) = find_node_by_name(&active_nodes, seed_name) {
                    resolved_seeds += 1;
                    let depth_map = bfs_traverse_with_depths(
                        &seed.id.to_string(),
                        &edge_types,
                        direction,
                        *expand_depth,
                        outgoing,
                        incoming,
                    );
                    for (id, d) in &depth_map {
                        let entry = all_depths.entry(id.clone()).or_insert(*d);
                        if *d < *entry {
                            *entry = *d;
                        }
                    }
                    result.extend(depth_map.into_keys());
                } else {
                    warnings.push(format!(
                        "Concept seed node '{}' not found in graph — check spelling or use qualified_name",
                        seed_name
                    ));
                }
            }
            // Warn when only some seed nodes resolved — the view may look
            // complete but is silently missing part of the intended concept.
            if resolved_seeds > 0 && resolved_seeds < total_seeds {
                warnings.push(format!(
                    "Concept scope partially resolved: {} of {} seed nodes found — results may be incomplete",
                    resolved_seeds, total_seeds
                ));
            }
            (result, all_depths)
        }
    };

    ScopeResult {
        matched,
        depths,
        warnings,
    }
}

/// Normalize whitespace in computed expressions.
/// Handles cases like "$callers (Foo)" → "$callers(Foo)" and extra internal spaces.
fn normalize_computed_expression(expr: &str) -> String {
    let mut result = String::with_capacity(expr.len());
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            // Read the function name
            result.push(chars[i]);
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                result.push(chars[i]);
                i += 1;
            }
            // Skip all ASCII whitespace before opening paren (space, tab, etc.)
            while i < chars.len() && chars[i].is_ascii_whitespace() {
                i += 1;
            }
            // If next char is '(', push it (we consumed the space)
            if i < chars.len() && chars[i] == '(' {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Attempt to resolve computed expressions.
/// Supports: $test_unreachable, $test_reachable, $clicked, $selected,
/// $where(...), $intersect(...), $diff(...), $union(...),
/// $callers(...), $callees(...), $implementors(...), $fields(...),
/// $descendants(...), $ancestors(...), $governed_by(...), $test_fragility(...),
/// $reachable(...)
/// Maximum recursion depth for resolver (mirrors validation MAX_COMPUTED_DEPTH).
const RESOLVER_MAX_DEPTH: u32 = 20;

fn resolve_computed_expression(
    expr: &str,
    active_nodes: &[&GraphNode],
    edges: &[GraphEdge],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
    selected_node_id: Option<&str>,
) -> HashSet<String> {
    resolve_computed_expression_inner(
        expr,
        active_nodes,
        edges,
        outgoing,
        incoming,
        selected_node_id,
        0,
    )
}

fn resolve_computed_expression_inner(
    expr: &str,
    active_nodes: &[&GraphNode],
    edges: &[GraphEdge],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
    selected_node_id: Option<&str>,
    depth: u32,
) -> HashSet<String> {
    if depth > RESOLVER_MAX_DEPTH {
        return HashSet::new(); // Prevent stack overflow on deeply nested expressions
    }

    // Normalize whitespace before parentheses: "$callers (Foo)" -> "$callers(Foo)"
    let normalized = normalize_computed_expression(expr);
    let trimmed = normalized.trim();

    // $clicked / $selected resolve to the selected node
    if trimmed == "$clicked" || trimmed == "$selected" {
        if let Some(id) = selected_node_id {
            let mut set = HashSet::new();
            set.insert(id.to_string());
            return set;
        }
        return HashSet::new();
    }

    if trimmed == "$test_unreachable" {
        let reachable = compute_test_reachable(active_nodes, outgoing);
        return active_nodes
            .iter()
            .filter(|n| !reachable.contains(&n.id.to_string()))
            .filter(|n| {
                matches!(
                    n.node_type,
                    NodeType::Function
                        | NodeType::Method
                        | NodeType::Endpoint
                        | NodeType::Type
                        | NodeType::Trait
                        | NodeType::Class
                )
            })
            .map(|n| n.id.to_string())
            .collect();
    }

    if trimmed == "$test_reachable" {
        return compute_test_reachable(active_nodes, outgoing);
    }

    // $where(property, op, value)
    if trimmed.starts_with("$where(") && trimmed.ends_with(')') {
        let inner = &trimmed[7..trimmed.len() - 1];
        let parts: Vec<&str> = inner
            .splitn(3, ',')
            .map(|s| s.trim().trim_matches('\'').trim_matches('"'))
            .collect();
        if parts.len() == 3 {
            let prop = parts[0];
            let op = parts[1];
            let val_str = parts[2];

            // Support string-valued properties (node_type, visibility, spec_confidence)
            let string_props = ["node_type", "visibility", "spec_confidence"];
            if string_props.contains(&prop) {
                let val_lower = val_str.to_lowercase();
                return active_nodes
                    .iter()
                    .filter(|n| {
                        let node_str = match prop {
                            "node_type" => node_type_str(&n.node_type).to_string(),
                            "visibility" => format!("{:?}", n.visibility).to_lowercase(),
                            "spec_confidence" => format!("{:?}", n.spec_confidence).to_lowercase(),
                            _ => String::new(),
                        };
                        match op {
                            "=" | "==" => node_str == val_lower,
                            "!=" => node_str != val_lower,
                            _ => false,
                        }
                    })
                    .map(|n| n.id.to_string())
                    .collect();
            }

            let val: f64 = match val_str.parse() {
                Ok(v) => v,
                Err(_) => {
                    // Invalid number — return empty set rather than silently treating as 0.0
                    return HashSet::new();
                }
            };
            // Precompute test fragility once (batch BFS) instead of per-node.
            let fragility_map = if prop == "test_fragility" {
                compute_all_test_fragility(active_nodes, outgoing, incoming)
            } else {
                HashMap::new()
            };
            return active_nodes
                .iter()
                .filter(|n| {
                    let nid = n.id.to_string();
                    let node_val = match prop {
                        "complexity" => n.complexity.map(|c| c as f64),
                        "churn" | "churn_count_30d" => Some(n.churn_count_30d as f64),
                        "test_coverage" => n.test_coverage,
                        "incoming_calls" => Some(count_incoming_calls(&nid, incoming) as f64),
                        "outgoing_calls" => Some(count_outgoing_calls(&nid, outgoing) as f64),
                        "field_count" => Some(count_fields(&nid, incoming) as f64),
                        "test_fragility" => Some(*fragility_map.get(&nid).unwrap_or(&0) as f64),
                        "risk_score" => {
                            // Only compute risk for nodes with actual complexity data;
                            // defaulting complexity=1 would inflate scores for unanalyzed nodes.
                            match n.complexity {
                                Some(c) => {
                                    let churn = n.churn_count_30d as f64;
                                    let complexity = c as f64;
                                    let test_gap = 1.0 - n.test_coverage.unwrap_or(0.0);
                                    Some(churn * complexity * test_gap)
                                }
                                None => None, // Exclude unanalyzed nodes from risk calculations
                            }
                        }
                        // Trace-based metrics require OTLP runtime data.
                        // Return 0.0 so nodes still appear in results; a warning is
                        // emitted below so callers know comparisons are meaningless.
                        "span_duration" | "span_count" | "error_rate" => Some(0.0),
                        _ => None,
                    };
                    match (node_val, op) {
                        (Some(nv), ">") => nv > val,
                        (Some(nv), ">=") => nv >= val,
                        (Some(nv), "<") => nv < val,
                        (Some(nv), "<=") => nv <= val,
                        (Some(nv), "==" | "=") => (nv - val).abs() < f64::EPSILON,
                        _ => false,
                    }
                })
                .map(|n| n.id.to_string())
                .collect();
        }
    }

    // $intersect(set_a, set_b)
    if trimmed.starts_with("$intersect(") && trimmed.ends_with(')') {
        let inner = &trimmed[11..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression_inner(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            let set_b = resolve_computed_expression_inner(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            return set_a.intersection(&set_b).cloned().collect();
        }
    }

    // $union(set_a, set_b)
    if trimmed.starts_with("$union(") && trimmed.ends_with(')') {
        let inner = &trimmed[7..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression_inner(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            let set_b = resolve_computed_expression_inner(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            return set_a.union(&set_b).cloned().collect();
        }
    }

    // $diff(set_a, set_b)
    if trimmed.starts_with("$diff(") && trimmed.ends_with(')') {
        let inner = &trimmed[6..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression_inner(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            let set_b = resolve_computed_expression_inner(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
                depth + 1,
            );
            return set_a.difference(&set_b).cloned().collect();
        }
    }

    // $callers(node_or_ref, depth?)
    if trimmed.starts_with("$callers(") && trimmed.ends_with(')') {
        let inner = &trimmed[9..trimmed.len() - 1];
        let (node_ref, depth) = split_node_and_depth(inner);

        let resolved_name = resolve_node_ref(&node_ref, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Calls],
                "incoming",
                depth,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $callees(node_or_ref, depth?)
    if trimmed.starts_with("$callees(") && trimmed.ends_with(')') {
        let inner = &trimmed[9..trimmed.len() - 1];
        let (node_ref, depth) = split_node_and_depth(inner);

        let resolved_name = resolve_node_ref(&node_ref, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Calls],
                "outgoing",
                depth,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $implementors(node)
    if trimmed.starts_with("$implementors(") && trimmed.ends_with(')') {
        let node_name = trimmed[14..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let resolved_name = resolve_node_ref(node_name, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            let found_id = found.id.to_string();
            return incoming
                .get(&found_id)
                .map(|neighbors| {
                    neighbors
                        .iter()
                        .filter(|(_, et)| *et == EdgeType::Implements)
                        .map(|(id, _)| id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
        }
        return HashSet::new();
    }

    // $fields(node)
    if trimmed.starts_with("$fields(") && trimmed.ends_with(')') {
        let node_name = trimmed[8..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let resolved_name = resolve_node_ref(node_name, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            let found_id = found.id.to_string();
            // FieldOf: source is the field, target is the parent type
            return incoming
                .get(&found_id)
                .map(|neighbors| {
                    neighbors
                        .iter()
                        .filter(|(_, et)| *et == EdgeType::FieldOf)
                        .map(|(id, _)| id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
        }
        return HashSet::new();
    }

    // $descendants(node) — cap depth at 100 (sufficient for any real hierarchy)
    if trimmed.starts_with("$descendants(") && trimmed.ends_with(')') {
        let node_name = trimmed[13..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let resolved_name = resolve_node_ref(node_name, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Contains],
                "outgoing",
                100,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $ancestors(node) — cap depth at 100 (sufficient for any real hierarchy)
    if trimmed.starts_with("$ancestors(") && trimmed.ends_with(')') {
        let node_name = trimmed[11..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let resolved_name = resolve_node_ref(node_name, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Contains],
                "incoming",
                100,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $governed_by(spec_path) — use GovernedBy edge traversal with exact path matching
    if trimmed.starts_with("$governed_by(") && trimmed.ends_with(')') {
        let spec_path = trimmed[13..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let lower = spec_path.to_lowercase();

        // Match helper: spec_path matches if it equals the query exactly,
        // ends with /query (directory boundary), or the basename matches exactly.
        // This prevents "auth.md" from matching "some-auth.md".
        let spec_matches = |sp: &str| -> bool {
            let sp_lower = sp.to_lowercase();
            if sp_lower == lower {
                return true;
            }
            // Require directory boundary: "specs/system/auth.md" matches "/auth.md"
            if sp_lower.ends_with(&format!("/{lower}")) {
                return true;
            }
            // Also match if the query is a full filename and matches the basename
            if lower.contains('/') {
                // Query has path separators — must match as suffix with boundary
                return false;
            }
            // Query is just a filename like "auth.md" — match only the basename
            let basename = sp_lower.rsplit('/').next().unwrap_or(&sp_lower);
            basename == lower
        };

        // First find spec nodes (or any node whose spec_path matches exactly)
        let spec_node_ids: HashSet<String> = active_nodes
            .iter()
            .filter(|n| n.spec_path.as_ref().map_or(false, |sp| spec_matches(sp)))
            .map(|n| n.id.to_string())
            .collect();

        // Traverse GovernedBy edges: source is the code node, target is the spec node
        let mut governed = HashSet::new();
        for edge in edges
            .iter()
            .filter(|e| e.deleted_at.is_none() && e.edge_type == EdgeType::GovernedBy)
        {
            if spec_node_ids.contains(&edge.target_id.to_string()) {
                governed.insert(edge.source_id.to_string());
            }
        }

        // When GovernedBy edges exist, also include the spec nodes themselves
        if !governed.is_empty() {
            governed.extend(spec_node_ids);
            return governed;
        }

        // Fallback: find code nodes with matching spec_path (exact match)
        // but do NOT return the spec nodes themselves — only code nodes governed by them.
        let code_nodes: HashSet<String> = active_nodes
            .iter()
            .filter(|n| {
                n.spec_path.as_ref().map_or(false, |sp| spec_matches(sp))
                    && n.node_type != NodeType::Spec
            })
            .map(|n| n.id.to_string())
            .collect();
        if !code_nodes.is_empty() {
            return code_nodes;
        }

        // No code nodes governed by this spec — return empty set rather than
        // the spec node itself, which would be misleading.
        // The caller should check the warnings for diagnostic information.
        return HashSet::new();
    }

    // $reachable(node, [edge_types], direction, depth) — general BFS traversal primitive.
    // Example: $reachable(MyService, [calls, implements], outgoing, 5)
    if trimmed.starts_with("$reachable(") && trimmed.ends_with(')') {
        let inner = &trimmed[11..trimmed.len() - 1];
        // Parse arguments: node, [edge_types], direction, depth
        // We need to handle the array inside, so we parse carefully
        let mut args: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut bracket_depth = 0;
        for ch in inner.chars() {
            match ch {
                '[' => {
                    bracket_depth += 1;
                    current.push(ch);
                }
                ']' => {
                    bracket_depth -= 1;
                    current.push(ch);
                }
                ',' if bracket_depth == 0 => {
                    args.push(current.trim().to_string());
                    current = String::new();
                }
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() {
            args.push(current.trim().to_string());
        }

        if args.len() >= 2 {
            let node_ref = args[0]
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_string();
            let edge_types_str = &args[1];
            let direction = if args.len() > 2 {
                args[2]
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string()
            } else {
                "outgoing".to_string()
            };
            let bfs_depth: u32 = if args.len() > 3 {
                args[3].trim().parse().unwrap_or(5)
            } else {
                5
            };

            // Parse edge types from array syntax: [calls, implements, ...]
            let edge_types: Vec<EdgeType> = edge_types_str
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .filter_map(|s| parse_edge_type(s.trim().trim_matches('\'').trim_matches('"')))
                .collect();

            if edge_types.is_empty() {
                return HashSet::new();
            }

            let resolved_name = resolve_node_ref(&node_ref, selected_node_id);
            if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
                return bfs_traverse(
                    &found.id.to_string(),
                    &edge_types,
                    &direction,
                    bfs_depth,
                    outgoing,
                    incoming,
                );
            }
        }
        return HashSet::new();
    }

    // $ungoverned — nodes with no GovernedBy edge and no spec_path set.
    // Useful for finding code that needs spec governance.
    if trimmed == "$ungoverned" {
        // Collect all nodes that are targets of GovernedBy edges (i.e., governed)
        let mut governed_ids: HashSet<String> = HashSet::new();
        for edge in edges
            .iter()
            .filter(|e| e.deleted_at.is_none() && e.edge_type == EdgeType::GovernedBy)
        {
            governed_ids.insert(edge.source_id.to_string());
        }
        // Also consider nodes with a spec_path as governed
        for n in active_nodes.iter() {
            if n.spec_path.is_some() {
                governed_ids.insert(n.id.to_string());
            }
        }
        // Return nodes that are NOT governed and are code nodes (not specs/modules)
        return active_nodes
            .iter()
            .filter(|n| {
                !governed_ids.contains(&n.id.to_string())
                    && n.node_type != NodeType::Spec
                    && n.node_type != NodeType::Module
            })
            .map(|n| n.id.to_string())
            .collect();
    }

    // $test_fragility(node) — set membership test: returns {node} if the node's
    // test_fragility count > 0. For scalar comparisons (e.g., filtering nodes with
    // fragility above a threshold), use $where(test_fragility, '>', N) instead.
    // The test_fragility metric is also available in $where() and heat-map emphasis.
    if trimmed.starts_with("$test_fragility(") && trimmed.ends_with(')') {
        let node_name = trimmed[16..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let resolved_name = resolve_node_ref(node_name, selected_node_id);
        if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
            let found_id = found.id.to_string();
            // Use batch computation (same work as single-node but consistent perf profile)
            let all_fragility = compute_all_test_fragility(active_nodes, outgoing, incoming);
            let count = all_fragility.get(&found_id).copied().unwrap_or(0);
            if count > 0 {
                let mut result = HashSet::new();
                result.insert(found_id);
                return result;
            }
        }
        return HashSet::new();
    }
    // NOTE: $test_fragility as a computed reference returns set membership (count > 0).
    // The actual fragility count is populated in node_metrics during dry_run
    // so that $where(test_fragility, '>', N) comparisons work with real counts.

    // Fallback: unrecognized expression — return empty set.
    // validate_computed_expression() should be called before resolution to catch these.
    #[cfg(debug_assertions)]
    eprintln!(
        "[view_query_resolver] Unrecognized computed expression: '{}'",
        trimmed
    );
    HashSet::new()
}

/// Validate a computed expression string and return any syntax errors.
/// Returns None if the expression is valid, Some(error) if not.
pub fn validate_computed_expression(expr: &str) -> Option<String> {
    let normalized = normalize_computed_expression(expr);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Some("Empty expression".to_string());
    }

    // Known top-level expressions
    let known_prefixes = [
        "$clicked",
        "$selected",
        "$test_unreachable",
        "$test_reachable",
        "$ungoverned",
        "$where(",
        "$intersect(",
        "$union(",
        "$diff(",
        "$callers(",
        "$callees(",
        "$implementors(",
        "$fields(",
        "$descendants(",
        "$ancestors(",
        "$governed_by(",
        "$test_fragility(",
        "$reachable(",
    ];

    // Standalone keywords must match exactly; parenthesized expressions match as prefix
    let is_known = known_prefixes.iter().any(|p| {
        if p.ends_with('(') {
            trimmed.starts_with(p)
        } else {
            trimmed == *p
        }
    });
    if !is_known {
        return Some(format!(
            "Unrecognized expression: '{trimmed}'. Known: $where, $callers, $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $test_fragility, $reachable, $intersect, $union, $diff, $test_unreachable, $test_reachable, $ungoverned, $clicked, $selected"
        ));
    }

    // Check balanced parentheses
    if trimmed.contains('(') {
        let open = trimmed.chars().filter(|c| *c == '(').count();
        let close = trimmed.chars().filter(|c| *c == ')').count();
        if open != close {
            return Some(format!(
                "Unbalanced parentheses in '{trimmed}': {open} open, {close} close"
            ));
        }
    }

    None
}

/// Resolve $clicked/$selected references to the actual node ID.
/// Split a `$callers`/`$callees` inner string into (node_ref, depth).
/// Handles generics like `HashMap<String, Vec<Task>>` by finding the last
/// top-level comma (not inside `<>`).
fn split_node_and_depth(inner: &str) -> (String, u32) {
    let inner = inner.trim();
    // Find the last comma that's NOT inside angle brackets
    let mut depth_bracket = 0i32;
    let mut last_top_comma = None;
    for (i, ch) in inner.char_indices() {
        match ch {
            '<' => depth_bracket += 1,
            '>' => {
                if depth_bracket > 0 {
                    depth_bracket -= 1;
                }
            }
            ',' if depth_bracket == 0 => last_top_comma = Some(i),
            _ => {}
        }
    }
    if let Some(comma_pos) = last_top_comma {
        let node_part = inner[..comma_pos].trim();
        let depth_part = inner[comma_pos + 1..].trim();
        if let Ok(d) = depth_part.parse::<u32>() {
            let node_ref = node_part.trim_matches('\'').trim_matches('"');
            return (node_ref.to_string(), d);
        }
    }
    // No depth separator found — entire thing is the node ref.
    // Default depth is 5 for reasonable traversal scope (not RESOLVER_MAX_DEPTH
    // which is the recursion limit for nested expressions, not traversal depth).
    let node_ref = inner.trim_matches('\'').trim_matches('"');
    (node_ref.to_string(), 5)
}

fn resolve_node_ref(reference: &str, selected_node_id: Option<&str>) -> String {
    match reference {
        "$clicked" | "$selected" => selected_node_id.unwrap_or("").to_string(),
        other => other.to_string(),
    }
}

/// Find a node by ID first, then fall back to name matching.
/// Accepts `&[&GraphNode]` to avoid cloning node slices.
/// Returns None for empty references (e.g. unresolved $clicked with no selection).
fn find_node_by_ref<'a>(nodes: &[&'a GraphNode], reference: &str) -> Option<&'a GraphNode> {
    if reference.is_empty() {
        return None;
    }
    // Try direct ID match first (for resolved $clicked/$selected).
    nodes
        .iter()
        .find(|n| n.id.to_string() == reference)
        .copied()
        .or_else(|| find_node_by_name(nodes, reference))
}

/// Split a string by commas, respecting balanced brackets and parentheses.
#[cfg(test)]
fn split_balanced_args(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut last = 0;

    for (i, c) in s.char_indices() {
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ',' if depth_paren == 0 && depth_bracket == 0 => {
                parts.push(&s[last..i]);
                last = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[last..]);
    parts
}

/// Find the position of the first comma that isn't inside balanced parentheses.
fn find_balanced_comma(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Resolve annotation template variables server-side.
/// `group_count` should be the number of distinct parent modules among matched nodes.
pub fn resolve_annotation_template(
    template: &str,
    focused_node_name: Option<&str>,
    matched_count: usize,
    group_count: usize,
) -> String {
    let mut result = template.to_string();
    if let Some(name) = focused_node_name {
        result = result.replace("$name", name);
    } else {
        // Remove unreplaceable $name rather than leaving literal "$name"
        result = result.replace("$name", "");
        // Clean up artifacts like "Blast radius: " → "Blast radius"
        result = result.replace(":  ", ": ");
        result = result.trim_end_matches(": ").to_string();
        result = result.trim_end_matches(':').to_string();
    }
    result = result.replace("{{count}}", &matched_count.to_string());
    result = result.replace("{{group_count}}", &group_count.to_string());
    result
}

/// Count distinct parent modules among a set of matched node IDs.
pub fn count_distinct_parent_modules(
    matched_ids: &HashSet<String>,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> usize {
    let mut parents = HashSet::new();
    for edge in edges.iter().filter(|e| e.deleted_at.is_none()) {
        if edge.edge_type == EdgeType::Contains && matched_ids.contains(&edge.target_id.to_string())
        {
            parents.insert(edge.source_id.to_string());
        }
    }
    // Fall back to counting distinct file_path prefixes if no Contains edges found
    if parents.is_empty() {
        for node in nodes
            .iter()
            .filter(|n| matched_ids.contains(&n.id.to_string()))
        {
            if let Some(last_sep) = node.file_path.rfind('/') {
                parents.insert(node.file_path[..last_sep].to_string());
            }
        }
    }
    parents.len()
}

// ── Dry-run ──────────────────────────────────────────────────────────────────

/// Run a complete dry-run of a view query, producing match counts and warnings.
pub fn dry_run(
    query: &ViewQuery,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
) -> DryRunResult {
    let mut warnings = Vec::new();

    // Validate ViewQuery fields (depth bounds, dim_unmatched range)
    let validation_errors = query.validate();
    warnings.extend(validation_errors);

    // Validate computed expressions before resolving (catch syntax errors)
    if let Scope::Filter {
        computed: Some(ref expr),
        ..
    } = &query.scope
    {
        if let Some(err) = validate_computed_expression(expr) {
            warnings.push(format!("Computed expression error: {err}"));
        }
    }

    // Build adjacency once — reused for scope resolution and heat metrics.
    let (adjacency_outgoing, adjacency_incoming) = build_adjacency(edges);

    // Resolve scope with depth info
    let scope_result = resolve_scope_with_adjacency(
        &query.scope,
        nodes,
        edges,
        selected_node_id,
        &adjacency_outgoing,
        &adjacency_incoming,
    );
    let result_set = scope_result.matched;
    let node_depths = scope_result.depths;
    warnings.extend(scope_result.warnings);

    // Warn when trace-based metrics are referenced anywhere in the query.
    // These metrics require OTLP runtime data that is not available in static
    // graph analysis, so all values will be 0.0 and comparisons are meaningless.
    {
        let trace_metrics = ["span_duration", "span_count", "error_rate"];
        let mut trace_in_use = false;
        if let Scope::Filter {
            computed: Some(ref expr),
            ..
        } = &query.scope
        {
            if trace_metrics.iter().any(|m| expr.contains(m)) {
                trace_in_use = true;
            }
        }
        if let Some(ref heat) = query.emphasis.heat {
            if trace_metrics.contains(&heat.metric.as_str()) {
                trace_in_use = true;
            }
        }
        if let Some(ref badges) = query.emphasis.badges {
            if let Some(ref metric) = badges.metric {
                if trace_metrics.contains(&metric.as_str()) {
                    trace_in_use = true;
                }
            }
        }
        if trace_in_use {
            warnings.push(
                "[warning] Trace metrics (span_duration, span_count, error_rate) require OTLP \
                 runtime telemetry data which is not present in the static graph model. All trace \
                 metric values are 0.0 and comparisons against them are meaningless. Use structural \
                 metrics instead: complexity, churn, incoming_calls, test_coverage, risk_score."
                    .to_string(),
            );
        }
    }

    if result_set.is_empty() {
        let is_interactive = matches!(&query.scope, Scope::Focus { node, .. } if node == "$clicked" || node == "$selected");
        if is_interactive && selected_node_id.is_none() {
            warnings
                .push("Interactive mode: click a node on the canvas to see results".to_string());
        } else if !is_interactive {
            warnings.push("Scope matched 0 nodes".to_string());
        }
    }
    if result_set.len() > 200 {
        warnings.push(format!(
            "Scope matched {} nodes - may be cluttered",
            result_set.len()
        ));
    }

    let node_map: HashMap<String, &GraphNode> = nodes
        .iter()
        .filter(|n| n.deleted_at.is_none())
        .map(|n| (n.id.to_string(), n))
        .collect();

    // Resolve the focused node name for annotation templates
    let focused_node_name: Option<String> = match &query.scope {
        Scope::Focus { node, .. } => {
            if node == "$clicked" || node == "$selected" {
                selected_node_id.and_then(|id| node_map.get(id).map(|n| n.name.clone()))
            } else {
                // Try to find the actual node name
                let active: Vec<&GraphNode> =
                    nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
                find_node_by_name(&active, node)
                    .map(|n| n.name.clone())
                    .or_else(|| Some(node.clone()))
            }
        }
        Scope::Concept { seed_nodes, .. } => {
            if seed_nodes.len() == 1 {
                Some(seed_nodes[0].clone())
            } else {
                None
            }
        }
        _ => None,
    };

    // Warn on partial (non-exact) node name matches in scope
    {
        let active: Vec<&GraphNode> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
        let scope_names: Vec<&str> = match &query.scope {
            Scope::Focus { node, .. } if node != "$clicked" && node != "$selected" => {
                vec![node.as_str()]
            }
            Scope::Concept { seed_nodes, .. } => seed_nodes.iter().map(|s| s.as_str()).collect(),
            _ => vec![],
        };
        for name in scope_names {
            if let Some((found, is_partial)) = find_node_by_name_with_match_type(&active, name) {
                if is_partial {
                    warnings.push(format!(
                        "Partial match: '{}' resolved to '{}' (not an exact match — use the qualified_name for precision)",
                        name, found.qualified_name
                    ));
                }
            }
        }
    }

    let total_matched = result_set.len();
    let matched_node_names: Vec<String> = result_set
        .iter()
        .filter_map(|id| node_map.get(id).map(|n| n.qualified_name.clone()))
        .take(50)
        .collect();
    if total_matched > 50 {
        warnings.push(format!(
            "Showing 50 of {} matched node names. The query may be too broad — consider adding filters.",
            total_matched
        ));
    }

    // Compute group_count as distinct parent modules (spec semantics)
    let group_count = count_distinct_parent_modules(&result_set, nodes, edges);

    // Resolve annotation templates with proper $name handling
    let has_unresolvable_name = focused_node_name.is_none()
        && (query
            .annotation
            .title
            .as_ref()
            .map_or(false, |t| t.contains("$name"))
            || query
                .annotation
                .description
                .as_ref()
                .map_or(false, |d| d.contains("$name")));
    if has_unresolvable_name {
        warnings.push(
            "Annotation uses $name but scope has no focused node (TestGaps, All, Filter, Diff scopes don't have one) — $name will be empty".to_string(),
        );
    }

    // Resolve annotation template variables server-side
    let resolved_annotation =
        if query.annotation.title.is_some() || query.annotation.description.is_some() {
            let resolved_title = query.annotation.title.as_ref().map(|t| {
                resolve_annotation_template(
                    t,
                    focused_node_name.as_deref(),
                    result_set.len(),
                    group_count,
                )
            });
            let resolved_desc = query.annotation.description.as_ref().map(|d| {
                resolve_annotation_template(
                    d,
                    focused_node_name.as_deref(),
                    result_set.len(),
                    group_count,
                )
            });
            Some(ResolvedAnnotation {
                title: resolved_title,
                description: resolved_desc,
            })
        } else {
            None
        };

    // Resolve groups — exact matching preferred, substring fallback only when
    // no exact match found (prevents "Task" matching "TaskPort", "TaskFilter", etc.)
    let mut groups_resolved = Vec::new();
    for group in &query.groups {
        let mut group_matched = Vec::new();
        for node_pattern in &group.nodes {
            let lower = node_pattern.to_lowercase();
            // Try exact match first (qualified_name or name)
            let exact: Vec<String> = node_map
                .iter()
                .filter(|(id, n)| {
                    result_set.contains(*id)
                        && (n.qualified_name.to_lowercase() == lower
                            || n.name.to_lowercase() == lower)
                })
                .map(|(_, n)| n.qualified_name.clone())
                .collect();
            if !exact.is_empty() {
                group_matched.extend(exact);
            } else {
                // Substring fallback only when no exact match
                for (id, n) in &node_map {
                    if result_set.contains(id)
                        && (n.qualified_name.to_lowercase().contains(&lower)
                            || n.name.to_lowercase().contains(&lower))
                    {
                        group_matched.push(n.qualified_name.clone());
                    }
                }
            }
        }
        if group_matched.len() > 20 {
            warnings.push(format!(
                "Group '{}' matched {} nodes - too broad",
                group.name,
                group_matched.len()
            ));
        }
        groups_resolved.push(ResolvedGroup {
            name: group.name.clone(),
            matched: group_matched.len(),
            nodes: group_matched,
        });
    }

    // Resolve callouts — prefer exact match, then prefix, then substring (with warning)
    let mut callouts_resolved = 0;
    let mut callouts_unresolved = Vec::new();
    for callout in &query.callouts {
        let lower = callout.node.to_lowercase();
        // 1. Exact match on name or qualified_name
        let exact = node_map.values().any(|n| {
            n.name.to_lowercase() == lower || n.qualified_name.to_lowercase() == lower
        });
        if exact {
            callouts_resolved += 1;
            continue;
        }
        // 2. Prefix match on name or qualified_name (name starts with the query)
        let prefix = node_map.values().any(|n| {
            n.name.to_lowercase().starts_with(&lower)
                || n.qualified_name.to_lowercase().starts_with(&lower)
        });
        if prefix {
            callouts_resolved += 1;
            continue;
        }
        // 3. Substring match (fallback with warning)
        let substring = node_map.values().any(|n| {
            n.qualified_name.to_lowercase().contains(&lower)
                || n.name.to_lowercase().contains(&lower)
        });
        if substring {
            callouts_resolved += 1;
            warnings.push(format!(
                "Callout node '{}' resolved via substring match — use the exact qualified_name for precision",
                callout.node
            ));
        } else {
            callouts_unresolved.push(callout.node.clone());
        }
    }
    if !callouts_unresolved.is_empty() {
        warnings.push(format!(
            "Unresolved callouts: {}. These nodes were not found in the graph.",
            callouts_unresolved.join(", ")
        ));
    }

    // Resolve narrative
    let mut narrative_resolved = 0;
    let mut narrative_unresolved = Vec::new();
    for step in &query.narrative {
        let lower = step.node.to_lowercase();
        let found = node_map.values().any(|n| {
            n.qualified_name.to_lowercase().contains(&lower)
                || n.name.to_lowercase().contains(&lower)
        });
        if found {
            narrative_resolved += 1;
        } else {
            narrative_unresolved.push(step.node.clone());
        }
    }
    if !narrative_unresolved.is_empty() {
        warnings.push(format!(
            "Unresolved narrative steps: {}. These nodes were not found in the graph.",
            narrative_unresolved.join(", ")
        ));
    }

    // Apply edge filtering: restrict edges to connections between matched nodes
    for et_str in &query.edges.filter {
        if parse_edge_type(et_str).is_none() {
            warnings.push(format!(
                "Unrecognized edge type '{}' in filter — ignored. Valid types: calls, contains, implements, depends_on, field_of, returns, routes_to, renders, persists_to, governed_by, produced_by",
                et_str
            ));
        }
    }
    let edge_type_filters: Vec<EdgeType> = query
        .edges
        .filter
        .iter()
        .filter_map(|s| parse_edge_type(s))
        .collect();
    const MAX_MATCHED_EDGES: usize = 1000;
    // First count the total matching edges for accurate reporting
    let total_matching_edges = edges
        .iter()
        .filter(|e| e.deleted_at.is_none())
        .filter(|e| {
            result_set.contains(&e.source_id.to_string())
                && result_set.contains(&e.target_id.to_string())
        })
        .filter(|e| edge_type_filters.is_empty() || edge_type_filters.contains(&e.edge_type))
        .count();
    let matched_edges: Vec<MatchedEdge> = edges
        .iter()
        .filter(|e| e.deleted_at.is_none())
        .filter(|e| {
            result_set.contains(&e.source_id.to_string())
                && result_set.contains(&e.target_id.to_string())
        })
        .filter(|e| edge_type_filters.is_empty() || edge_type_filters.contains(&e.edge_type))
        .take(MAX_MATCHED_EDGES)
        .map(|e| MatchedEdge {
            source_id: e.source_id.to_string(),
            target_id: e.target_id.to_string(),
            edge_type: edge_type_str(&e.edge_type).to_string(),
        })
        .collect();
    if total_matching_edges > MAX_MATCHED_EDGES {
        warnings.push(format!(
            "Matched edges capped at {MAX_MATCHED_EDGES} (total: {} edges before truncation)",
            total_matching_edges
        ));
    }

    // Compute per-node metric values for heat emphasis (reuses pre-built adjacency)
    let mut node_metrics = if let Some(ref heat) = query.emphasis.heat {
        // Warn about trace-based metrics that require OTLP runtime data
        match heat.metric.as_str() {
            "span_duration" | "span_count" | "error_rate" => {
                warnings.push(format!(
                    "[info] Heat metric '{}' requires OTLP trace data which is not available in the graph model",
                    heat.metric
                ));
            }
            _ => {}
        }
        // Pre-compute test fragility for all nodes at once if needed
        let fragility_map = if heat.metric == "test_fragility" {
            let active_nodes: Vec<&GraphNode> =
                nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
            compute_all_test_fragility(&active_nodes, &adjacency_outgoing, &adjacency_incoming)
        } else {
            HashMap::new()
        };
        result_set
            .iter()
            .filter_map(|id| {
                let val = match heat.metric.as_str() {
                    "complexity" => node_map
                        .get(id)
                        .and_then(|n| n.complexity.map(|c| c as f64)),
                    "churn" | "churn_count_30d" => {
                        node_map.get(id).map(|n| n.churn_count_30d as f64)
                    }
                    "incoming_calls" => Some(count_incoming_calls(id, &adjacency_incoming) as f64),
                    "outgoing_calls" => Some(count_outgoing_calls(id, &adjacency_outgoing) as f64),
                    "test_coverage" => node_map.get(id).and_then(|n| n.test_coverage),
                    "field_count" => Some(count_fields(id, &adjacency_incoming) as f64),
                    "test_fragility" => Some(*fragility_map.get(id).unwrap_or(&0) as f64),
                    "risk_score" => {
                        // Composite risk: churn × complexity × (1 - test_coverage)
                        // Only compute for nodes with actual complexity data — consistent
                        // with $where(risk_score, ...) which returns None for unanalyzed nodes.
                        node_map.get(id).and_then(|n| {
                            n.complexity.map(|c| {
                                let churn = n.churn_count_30d as f64;
                                let complexity = c as f64;
                                let test_gap = 1.0 - n.test_coverage.unwrap_or(0.0);
                                churn * complexity * test_gap
                            })
                        })
                    }
                    // Trace-based metrics return 0.0 placeholder so nodes still
                    // appear in results (warning already emitted above).
                    "span_duration" | "span_count" | "error_rate" => Some(0.0),
                    _ => {
                        if !heat.metric.is_empty() {
                            // Log unrecognized metric once via warning (already validated)
                        }
                        None
                    }
                };
                val.map(|v| (id.clone(), v))
            })
            .collect()
    } else {
        HashMap::new()
    };

    // Populate node_metrics with test_fragility counts when $test_fragility is
    // used in a computed expression, so $where(test_fragility, '>', N) can
    // compare actual counts rather than just boolean membership.
    if let Scope::Filter {
        computed: Some(ref expr),
        ..
    } = &query.scope
    {
        if expr.contains("$test_fragility") && !node_metrics.iter().any(|(_, v)| *v > 0.0) {
            let active_nodes: Vec<&GraphNode> =
                nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
            let fragility_map =
                compute_all_test_fragility(&active_nodes, &adjacency_outgoing, &adjacency_incoming);
            for id in &result_set {
                let count = fragility_map.get(id).copied().unwrap_or(0);
                node_metrics.entry(id.clone()).or_insert(count as f64);
            }
        }
    }

    DryRunResult {
        matched_nodes: result_set.len(),
        matched_node_names,
        groups_resolved,
        callouts_resolved,
        callouts_unresolved,
        narrative_resolved,
        warnings,
        node_depths,
        node_metrics,
        matched_edges,
        resolved_annotation,
    }
}

// ── Graph summary ────────────────────────────────────────────────────────────

/// Compute a condensed summary of a repo's knowledge graph.
pub fn compute_graph_summary(
    repo_id: &str,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> GraphSummary {
    let active_nodes: Vec<&GraphNode> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
    let active_edges: Vec<&GraphEdge> = edges.iter().filter(|e| e.deleted_at.is_none()).collect();

    // Count by type
    let mut node_counts: HashMap<String, usize> = HashMap::new();
    for n in &active_nodes {
        *node_counts
            .entry(node_type_str(&n.node_type).to_string())
            .or_default() += 1;
    }

    let mut edge_counts: HashMap<String, usize> = HashMap::new();
    for e in &active_edges {
        *edge_counts
            .entry(edge_type_str(&e.edge_type).to_string())
            .or_default() += 1;
    }

    // Top types by field count (FieldOf: source=field, target=parent_type)
    let mut field_counts: HashMap<String, usize> = HashMap::new();
    for e in &active_edges {
        if e.edge_type == EdgeType::FieldOf {
            *field_counts.entry(e.target_id.to_string()).or_default() += 1;
        }
    }
    let node_map: HashMap<String, &GraphNode> = active_nodes
        .iter()
        .map(|n| (n.id.to_string(), *n))
        .collect();
    let mut type_field_pairs: Vec<(String, usize)> = field_counts
        .iter()
        .filter_map(|(id, count)| {
            node_map
                .get(id)
                .map(|n| (format!("{} ({} fields)", n.name, count), *count))
        })
        .collect();
    type_field_pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let top_types: Vec<String> = type_field_pairs
        .into_iter()
        .take(10)
        .map(|(s, _)| s)
        .collect();

    // Top functions by incoming calls
    let mut call_counts: HashMap<String, usize> = HashMap::new();
    for e in &active_edges {
        if e.edge_type == EdgeType::Calls {
            *call_counts.entry(e.target_id.to_string()).or_default() += 1;
        }
    }
    let mut func_call_pairs: Vec<(String, usize)> = call_counts
        .iter()
        .filter_map(|(id, count)| {
            node_map
                .get(id)
                .map(|n| (format!("{} ({})", n.name, count), *count))
        })
        .collect();
    func_call_pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let top_functions: Vec<String> = func_call_pairs
        .iter()
        .take(10)
        .map(|(s, _)| s.clone())
        .collect();

    // Modules
    let modules: Vec<String> = active_nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Module)
        .map(|n| n.qualified_name.clone())
        .collect();

    // Test coverage
    let (outgoing, _) = build_adjacency(edges);
    let test_functions = active_nodes.iter().filter(|n| n.test_node).count();
    let reachable = compute_test_reachable(&active_nodes, &outgoing);
    // Include Functions, Methods, and Endpoints in test coverage denominator,
    // matching what TestGaps scope considers as testable nodes.
    let total_functions = active_nodes
        .iter()
        .filter(|n| {
            matches!(
                n.node_type,
                NodeType::Function | NodeType::Method | NodeType::Endpoint
            )
        })
        .count();
    let reachable_count = active_nodes
        .iter()
        .filter(|n| {
            matches!(
                n.node_type,
                NodeType::Function | NodeType::Method | NodeType::Endpoint
            ) && reachable.contains(&n.id.to_string())
        })
        .count();

    // Spec coverage stats
    let governed = active_nodes
        .iter()
        .filter(|n| n.spec_path.is_some() || n.spec_confidence != SpecConfidence::None)
        .count();
    let spec_total = active_nodes.len();
    let unspecced = spec_total.saturating_sub(governed);

    // Compute percentage fields
    let spec_coverage_pct = if spec_total > 0 {
        (governed as f64 / spec_total as f64) * 100.0
    } else {
        0.0
    };
    let test_coverage_pct = if total_functions > 0 {
        (reachable_count as f64 / total_functions as f64) * 100.0
    } else {
        0.0
    };

    // Risk indicators — top anomalies the LLM should know about
    let mut risk_indicators = Vec::new();
    let high_complexity_untested: Vec<&str> = active_nodes
        .iter()
        .filter(|n| {
            n.complexity.unwrap_or(0) > 20
                && n.node_type == NodeType::Function
                && !reachable.contains(&n.id.to_string())
        })
        .take(5)
        .map(|n| n.name.as_str())
        .collect();
    if !high_complexity_untested.is_empty() {
        risk_indicators.push(format!(
            "High-complexity untested functions: {}",
            high_complexity_untested.join(", ")
        ));
    }
    // Hot unspecced code: heavily-called functions with no governing spec
    let hot_unspecced: Vec<String> = func_call_pairs
        .iter()
        .take(20)
        .filter_map(|(label, count)| {
            if *count < 3 {
                return None;
            }
            // Extract the function name from the label "FuncName (N)"
            let name = label.split(" (").next()?;
            let node = active_nodes.iter().find(|n| n.name == name)?;
            if node.spec_path.is_some() || node.spec_confidence != SpecConfidence::None {
                return None;
            }
            Some(format!("{name} ({count} callers, no spec)"))
        })
        .take(5)
        .collect();
    if !hot_unspecced.is_empty() {
        risk_indicators.push(format!(
            "Heavily-called unspecced code: {}",
            hot_unspecced.join(", ")
        ));
    }

    // High-risk nodes: complexity > p90 AND low test coverage
    // Compute p90 complexity threshold from all functions with complexity data
    let mut complexities: Vec<u32> = active_nodes
        .iter()
        .filter(|n| matches!(n.node_type, NodeType::Function | NodeType::Method))
        .filter_map(|n| n.complexity)
        .filter(|c| *c > 0)
        .collect();
    if !complexities.is_empty() {
        complexities.sort_unstable();
        let p90_idx = (complexities.len() as f64 * 0.9).ceil() as usize;
        let p90_threshold = complexities[p90_idx.min(complexities.len() - 1)];
        let high_risk: Vec<String> = active_nodes
            .iter()
            .filter(|n| matches!(n.node_type, NodeType::Function | NodeType::Method))
            .filter(|n| n.complexity.unwrap_or(0) >= p90_threshold)
            .filter(|n| {
                // Low coverage: not reachable from tests OR test_coverage < 0.3
                !reachable.contains(&n.id.to_string())
                    || n.test_coverage.map_or(false, |tc| tc < 0.3)
            })
            .take(10)
            .map(|n| {
                format!(
                    "{} (complexity: {}, coverage: {})",
                    n.name,
                    n.complexity.unwrap_or(0),
                    n.test_coverage
                        .map_or("none".to_string(), |c| format!("{:.0}%", c * 100.0))
                )
            })
            .collect();
        if !high_risk.is_empty() {
            risk_indicators.push(format!(
                "High-risk (complexity >= p90={}, low coverage): {}",
                p90_threshold,
                high_risk.join(", ")
            ));
        }
    }

    // Anomaly summary: quick counts for common questions
    let high_complexity_count = active_nodes
        .iter()
        .filter(|n| {
            matches!(n.node_type, NodeType::Function | NodeType::Method)
                && n.complexity.unwrap_or(0) > 20
        })
        .count();

    // Orphan functions: functions with no incoming or outgoing call edges
    let functions_with_calls: HashSet<String> = active_edges
        .iter()
        .filter(|e| e.edge_type == EdgeType::Calls)
        .flat_map(|e| vec![e.source_id.to_string(), e.target_id.to_string()])
        .collect();
    let orphan_function_count = active_nodes
        .iter()
        .filter(|n| {
            matches!(n.node_type, NodeType::Function | NodeType::Method)
                && !n.test_node
                && !functions_with_calls.contains(&n.id.to_string())
        })
        .count();

    let untested_function_count = total_functions.saturating_sub(reachable_count);

    let anomaly_summary = AnomalySummary {
        high_complexity_count,
        orphan_function_count,
        untested_function_count,
    };

    // Recent changes: nodes modified in the last 7 days
    let seven_days_ago = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(7 * 24 * 3600)
    };
    let recent_changes_7d = active_nodes
        .iter()
        .filter(|n| n.last_modified_at > seven_days_ago)
        .count();

    GraphSummary {
        repo_id: repo_id.to_string(),
        node_counts,
        edge_counts,
        top_types_by_fields: top_types,
        top_functions_by_calls: top_functions,
        modules,
        test_coverage: TestCoverageSummary {
            test_functions,
            reachable_from_tests: reachable_count,
            unreachable: total_functions.saturating_sub(reachable_count),
        },
        spec_coverage: SpecCoverageSummary {
            governed,
            unspecced,
            total: spec_total,
        },
        spec_coverage_pct,
        test_coverage_pct,
        risk_indicators,
        anomaly_summary,
        recent_changes_7d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::*;
    use gyre_common::Id;

    fn make_node(id: &str, name: &str, node_type: NodeType) -> GraphNode {
        GraphNode {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            node_type,
            name: name.to_string(),
            qualified_name: format!("pkg.{}", name),
            file_path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 10,
            visibility: Visibility::Public,
            doc_comment: None,
            spec_path: None,
            spec_paths: vec![],
            spec_confidence: SpecConfidence::None,
            last_modified_sha: "abc123".to_string(),
            last_modified_by: None,
            last_modified_at: 1000,
            created_sha: "abc123".to_string(),
            created_at: 1000,
            complexity: Some(5),
            churn_count_30d: 2,
            test_coverage: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
            test_node: false,
            spec_approved_at: None,
            milestone_completed_at: None,
        }
    }

    fn make_test_node(id: &str, name: &str) -> GraphNode {
        let mut n = make_node(id, name, NodeType::Function);
        n.test_node = true;
        n
    }

    fn make_edge(id: &str, source: &str, target: &str, edge_type: EdgeType) -> GraphEdge {
        GraphEdge {
            id: Id::new(id),
            repo_id: Id::new("repo1"),
            source_id: Id::new(source),
            target_id: Id::new(target),
            edge_type,
            metadata: None,
            first_seen_at: 1000,
            last_seen_at: 1000,
            deleted_at: None,
        }
    }

    #[test]
    fn test_scope_all() {
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "Bar", NodeType::Type),
        ];
        let edges = vec![];
        let result = resolve_scope(&Scope::All, &nodes, &edges, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_scope_filter_by_type() {
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "bar_fn", NodeType::Function),
            make_node("n3", "Baz", NodeType::Type),
        ];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec!["type".to_string()],
                computed: None,
                name_pattern: None,
            },
            &nodes,
            &[],
            None,
        );
        assert_eq!(result.len(), 2);
        assert!(result.contains("n1"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_scope_filter_by_name_pattern() {
        let nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "auth_handler", NodeType::Function),
            make_node("n3", "UserRepo", NodeType::Type),
        ];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: None,
                name_pattern: Some("auth".to_string()),
            },
            &nodes,
            &[],
            None,
        );
        assert_eq!(result.len(), 2);
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(!result.contains("n3"));
    }

    #[test]
    fn test_scope_focus_bfs() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
            make_node("n4", "D", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
            make_edge("e3", "n3", "n4", EdgeType::Calls),
        ];
        let result = resolve_scope(
            &Scope::Focus {
                node: "A".to_string(),
                edges: vec!["calls".to_string()],
                direction: "outgoing".to_string(),
                depth: 2,
            },
            &nodes,
            &edges,
            None,
        );
        assert_eq!(result.len(), 3);
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
        assert!(!result.contains("n4"));
    }

    #[test]
    fn test_scope_focus_incoming() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n3", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
        ];
        let result = resolve_scope(
            &Scope::Focus {
                node: "C".to_string(),
                edges: vec!["calls".to_string()],
                direction: "incoming".to_string(),
                depth: 5,
            },
            &nodes,
            &edges,
            None,
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_scope_test_gaps() {
        let nodes = vec![
            make_node("n1", "tested_fn", NodeType::Function),
            make_node("n2", "untested_fn", NodeType::Function),
            make_test_node("t1", "test_something"),
        ];
        let edges = vec![make_edge("e1", "t1", "n1", EdgeType::Calls)];
        let result = resolve_scope(&Scope::TestGaps, &nodes, &edges, None);
        assert!(result.contains("n2"));
        assert!(!result.contains("n1"));
    }

    #[test]
    fn test_scope_test_gaps_includes_endpoints_and_types() {
        let nodes = vec![
            make_node("n1", "tested_fn", NodeType::Function),
            make_node("n2", "untested_endpoint", NodeType::Endpoint),
            make_node("n3", "untested_type", NodeType::Type),
            make_node("n4", "untested_module", NodeType::Module), // Modules excluded
            make_test_node("t1", "test_something"),
        ];
        let edges = vec![make_edge("e1", "t1", "n1", EdgeType::Calls)];
        let result = resolve_scope(&Scope::TestGaps, &nodes, &edges, None);
        assert!(
            result.contains("n2"),
            "Endpoints should appear in test gaps"
        );
        assert!(result.contains("n3"), "Types should appear in test gaps");
        assert!(
            !result.contains("n4"),
            "Modules should NOT appear in test gaps"
        );
    }

    #[test]
    fn test_scope_concept() {
        let nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "TokenStore", NodeType::Type),
            make_node("n3", "LoginHandler", NodeType::Function),
            make_node("n4", "Unrelated", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n3", "n1", EdgeType::Calls),
        ];
        let result = resolve_scope(
            &Scope::Concept {
                seed_nodes: vec!["AuthService".to_string()],
                expand_edges: vec!["calls".to_string()],
                expand_depth: 2,
                expand_direction: "both".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
        assert!(!result.contains("n4"));
    }

    #[test]
    fn test_computed_test_unreachable() {
        let nodes = vec![
            make_node("n1", "reachable", NodeType::Function),
            make_node("n2", "unreachable", NodeType::Function),
            make_test_node("t1", "test_fn"),
        ];
        let edges = vec![make_edge("e1", "t1", "n1", EdgeType::Calls)];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$test_unreachable",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n2"));
        assert!(!result.contains("n1"));
    }

    #[test]
    fn test_computed_where() {
        let mut nodes = vec![
            make_node("n1", "complex_fn", NodeType::Function),
            make_node("n2", "simple_fn", NodeType::Function),
        ];
        nodes[0].complexity = Some(30);
        nodes[1].complexity = Some(5);
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&[]);
        let result = resolve_computed_expression(
            "$where(complexity, '>', 20)",
            &active,
            &[],
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(!result.contains("n2"));
    }

    #[test]
    fn test_computed_where_incoming_calls() {
        let nodes = vec![
            make_node("n1", "popular_fn", NodeType::Function),
            make_node("n2", "lonely_fn", NodeType::Function),
            make_node("n3", "caller1", NodeType::Function),
            make_node("n4", "caller2", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n3", "n1", EdgeType::Calls),
            make_edge("e2", "n4", "n1", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$where(incoming_calls, '>=', 2)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(!result.contains("n2"));
    }

    #[test]
    fn test_computed_intersect() {
        let mut nodes = vec![
            make_node("n1", "complex_untested", NodeType::Function),
            make_node("n2", "simple_untested", NodeType::Function),
            make_node("n3", "complex_tested", NodeType::Function),
            make_test_node("t1", "test_fn"),
        ];
        nodes[0].complexity = Some(30);
        nodes[1].complexity = Some(5);
        nodes[2].complexity = Some(25);
        let edges = vec![make_edge("e1", "t1", "n3", EdgeType::Calls)];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$intersect($where(complexity, '>', 20), $test_unreachable)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(!result.contains("n2"));
        assert!(!result.contains("n3"));
    }

    #[test]
    fn test_dry_run_basic() {
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "Bar", NodeType::Function),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert_eq!(result.matched_nodes, 2);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_dry_run_warns_on_many_nodes() {
        let nodes: Vec<GraphNode> = (0..250)
            .map(|i| make_node(&format!("n{i}"), &format!("Fn{i}"), NodeType::Function))
            .collect();
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert_eq!(result.matched_nodes, 250);
        assert!(result.warnings.iter().any(|w| w.contains("250 nodes")));
    }

    #[test]
    fn test_graph_summary() {
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "bar_fn", NodeType::Function),
            make_node("n3", "baz_fn", NodeType::Function),
            make_test_node("t1", "test_bar"),
            make_node("m1", "main_mod", NodeType::Module),
        ];
        let edges = vec![
            make_edge("e1", "n2", "n3", EdgeType::Calls),
            make_edge("e2", "t1", "n2", EdgeType::Calls),
        ];
        let summary = compute_graph_summary("repo1", &nodes, &edges);
        assert_eq!(*summary.node_counts.get("type").unwrap_or(&0), 1);
        assert_eq!(*summary.node_counts.get("function").unwrap_or(&0), 3);
        assert_eq!(*summary.edge_counts.get("calls").unwrap_or(&0), 2);
        assert_eq!(summary.test_coverage.test_functions, 1);
        assert_eq!(summary.modules.len(), 1);
    }

    #[test]
    fn test_computed_callers() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n3", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result =
            resolve_computed_expression("$callers(C)", &active, &edges, &outgoing, &incoming, None);
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_callers_with_selected() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n3", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        // $callers($selected) should resolve to C when selected_node_id = "n3"
        let result = resolve_computed_expression(
            "$callers($selected)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            Some("n3"),
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_callees() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n1", "n3", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result =
            resolve_computed_expression("$callees(A)", &active, &edges, &outgoing, &incoming, None);
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_implementors() {
        let nodes = vec![
            make_node("n1", "MyTrait", NodeType::Interface),
            make_node("n2", "ImplA", NodeType::Type),
            make_node("n3", "ImplB", NodeType::Type),
        ];
        let edges = vec![
            make_edge("e1", "n2", "n1", EdgeType::Implements),
            make_edge("e2", "n3", "n1", EdgeType::Implements),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$implementors(MyTrait)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
        assert!(!result.contains("n1"));
    }

    #[test]
    fn test_computed_fields() {
        let nodes = vec![
            make_node("n1", "MyType", NodeType::Type),
            make_node("n2", "field_a", NodeType::Field),
            make_node("n3", "field_b", NodeType::Field),
        ];
        let edges = vec![
            make_edge("e1", "n2", "n1", EdgeType::FieldOf),
            make_edge("e2", "n3", "n1", EdgeType::FieldOf),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$fields(MyType)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
        assert!(!result.contains("n1"));
    }

    #[test]
    fn test_computed_descendants() {
        let nodes = vec![
            make_node("n1", "root_mod", NodeType::Module),
            make_node("n2", "child_fn", NodeType::Function),
            make_node("n3", "grandchild", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Contains),
            make_edge("e2", "n2", "n3", EdgeType::Contains),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$descendants(root_mod)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_ancestors() {
        let nodes = vec![
            make_node("n1", "root_mod", NodeType::Module),
            make_node("n2", "child_mod", NodeType::Module),
            make_node("n3", "leaf_fn", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Contains),
            make_edge("e2", "n2", "n3", EdgeType::Contains),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$ancestors(leaf_fn)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_governed_by_with_edges() {
        let mut nodes = vec![
            make_node("n1", "SearchService", NodeType::Type),
            make_node("n2", "AuthService", NodeType::Type),
            make_node("n3", "search_spec", NodeType::Type),
        ];
        nodes[2].spec_path = Some("specs/search.md".to_string());
        let edges = vec![
            // SearchService is governed by search_spec
            make_edge("e1", "n1", "n3", EdgeType::GovernedBy),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$governed_by(search.md)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1")); // governed via edge
        assert!(result.contains("n3")); // spec node itself
        assert!(!result.contains("n2")); // not governed
    }

    #[test]
    fn test_computed_test_fragility() {
        let nodes = vec![
            make_node("n1", "important_fn", NodeType::Function),
            make_node("n2", "unused_fn", NodeType::Function),
            make_test_node("t1", "test_a"),
            make_test_node("t2", "test_b"),
        ];
        let edges = vec![
            make_edge("e1", "t1", "n1", EdgeType::Calls),
            make_edge("e2", "t2", "n1", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        // important_fn is reachable by 2 tests, so it should be in the result
        let result = resolve_computed_expression(
            "$test_fragility(important_fn)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        // unused_fn has 0 test paths
        let result2 = resolve_computed_expression(
            "$test_fragility(unused_fn)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result2.is_empty());
    }

    #[test]
    fn test_computed_union_and_diff() {
        let nodes = vec![
            make_node("n1", "TypeA", NodeType::Type),
            make_node("n2", "FuncB", NodeType::Function),
            make_node("n3", "TypeC", NodeType::Type),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&[]);

        let union = resolve_computed_expression(
            "$union($where(complexity, '>=', 0), $where(complexity, '<', 0))",
            &active,
            &[],
            &outgoing,
            &incoming,
            None,
        );
        assert!(union.contains("n1"));
        assert!(union.contains("n2"));
        assert!(union.contains("n3"));

        let mut nodes_varied = nodes.clone();
        nodes_varied[0].complexity = Some(30);
        nodes_varied[1].complexity = Some(3);
        nodes_varied[2].complexity = Some(25);
        let active2: Vec<&GraphNode> = nodes_varied.iter().collect();
        let diff = resolve_computed_expression(
            "$diff($where(complexity, '>', 0), $where(complexity, '>', 20))",
            &active2,
            &[],
            &outgoing,
            &incoming,
            None,
        );
        assert!(diff.contains("n2"));
        assert!(!diff.contains("n1"));
    }

    #[test]
    fn test_computed_reachable_general() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$reachable(A, [calls], outgoing, 5)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_reachable_multi_edge_types() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Type),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Implements),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$reachable(A, [calls, implements], outgoing, 5)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_deleted_nodes_excluded() {
        let mut n = make_node("n1", "Deleted", NodeType::Type);
        n.deleted_at = Some(2000);
        let nodes = vec![n, make_node("n2", "Active", NodeType::Type)];
        let result = resolve_scope(&Scope::All, &nodes, &[], None);
        assert_eq!(result.len(), 1);
        assert!(result.contains("n2"));
    }

    #[test]
    fn test_annotation_template_resolution() {
        let resolved = resolve_annotation_template(
            "Blast radius: $name ({{count}} nodes, {{group_count}} groups)",
            Some("TaskPort"),
            14,
            3,
        );
        assert_eq!(resolved, "Blast radius: TaskPort (14 nodes, 3 groups)");
    }

    #[test]
    fn test_annotation_template_no_focus_node() {
        // When there's no focused node, $name should be removed (not left as literal "$name")
        let resolved =
            resolve_annotation_template("Test gaps: $name ({{count}} nodes)", None, 42, 5);
        assert!(
            !resolved.contains("$name"),
            "Literal $name should not remain"
        );
        assert!(resolved.contains("42 nodes"));
    }

    #[test]
    fn test_focus_scope_returns_depths() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
        ];
        let result = resolve_scope_with_depths(
            &Scope::Focus {
                node: "A".to_string(),
                edges: vec!["calls".to_string()],
                direction: "outgoing".to_string(),
                depth: 5,
            },
            &nodes,
            &edges,
            None,
        );
        assert_eq!(*result.depths.get("n1").unwrap(), 0);
        assert_eq!(*result.depths.get("n2").unwrap(), 1);
        assert_eq!(*result.depths.get("n3").unwrap(), 2);
    }

    #[test]
    fn test_dry_run_edge_filtering() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n1", "n2", EdgeType::Contains),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: gyre_common::view_query::EdgeFilter {
                filter: vec!["calls".to_string()],
            },
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert_eq!(result.matched_edges.len(), 1);
        assert_eq!(result.matched_edges[0].edge_type, "calls");
    }

    #[test]
    fn test_dry_run_node_depths_for_focus() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
        ];
        let edges = vec![make_edge("e1", "n1", "n2", EdgeType::Calls)];
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "A".to_string(),
                edges: vec!["calls".to_string()],
                direction: "outgoing".to_string(),
                depth: 5,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert!(
            !result.node_depths.is_empty(),
            "Focus scope should return depths"
        );
        assert_eq!(*result.node_depths.get("n1").unwrap(), 0);
        assert_eq!(*result.node_depths.get("n2").unwrap(), 1);
    }

    #[test]
    fn test_split_balanced_args() {
        let args = split_balanced_args("A, [calls, implements], outgoing, 5");
        assert_eq!(args.len(), 4);
        assert_eq!(args[0].trim(), "A");
        assert_eq!(args[1].trim(), "[calls, implements]");
        assert_eq!(args[2].trim(), "outgoing");
        assert_eq!(args[3].trim(), "5");
    }

    // ── validate_computed_expression tests ──────────────────────────────────

    #[test]
    fn test_validate_computed_expression_valid() {
        // All recognized expressions should return None (no error)
        assert!(validate_computed_expression("$clicked").is_none());
        assert!(validate_computed_expression("$selected").is_none());
        assert!(validate_computed_expression("$test_unreachable").is_none());
        assert!(validate_computed_expression("$test_reachable").is_none());
        assert!(validate_computed_expression("$where(complexity, '>', 10)").is_none());
        assert!(validate_computed_expression("$callers(A)").is_none());
        assert!(validate_computed_expression("$callees(B)").is_none());
        assert!(validate_computed_expression("$implementors(MyTrait)").is_none());
        assert!(validate_computed_expression("$fields(MyType)").is_none());
        assert!(validate_computed_expression("$descendants(root)").is_none());
        assert!(validate_computed_expression("$ancestors(leaf)").is_none());
        assert!(validate_computed_expression("$governed_by(spec.md)").is_none());
        assert!(validate_computed_expression("$test_fragility(fn_name)").is_none());
        assert!(validate_computed_expression("$reachable(A, [calls], outgoing, 5)").is_none());
        assert!(validate_computed_expression(
            "$intersect($where(complexity, '>', 10), $test_unreachable)"
        )
        .is_none());
        assert!(validate_computed_expression("$union($callers(A), $callees(B))").is_none());
        assert!(validate_computed_expression("$diff($test_reachable, $callers(C))").is_none());
    }

    #[test]
    fn test_validate_computed_expression_invalid() {
        // Empty expression
        let err = validate_computed_expression("").unwrap();
        assert!(err.contains("Empty expression"));

        // Unrecognized expression
        let err = validate_computed_expression("$bogus(x)").unwrap();
        assert!(err.contains("Unrecognized expression"));
        assert!(err.contains("$bogus(x)"));

        // Plain text (not a $ expression)
        let err = validate_computed_expression("just some text").unwrap();
        assert!(err.contains("Unrecognized expression"));
    }

    #[test]
    fn test_validate_computed_expression_unbalanced_parens() {
        let err = validate_computed_expression("$where(complexity, '>', 10").unwrap();
        assert!(err.contains("Unbalanced parentheses"));
        assert!(err.contains("1 open, 0 close"));
    }

    // ── count_distinct_parent_modules tests ─────────────────────────────────

    #[test]
    fn test_count_distinct_parent_modules_via_contains_edges() {
        let nodes = vec![
            make_node("n1", "fn_a", NodeType::Function),
            make_node("n2", "fn_b", NodeType::Function),
            make_node("m1", "mod_a", NodeType::Module),
            make_node("m2", "mod_b", NodeType::Module),
        ];
        let edges = vec![
            make_edge("e1", "m1", "n1", EdgeType::Contains),
            make_edge("e2", "m2", "n2", EdgeType::Contains),
        ];
        let mut matched = HashSet::new();
        matched.insert("n1".to_string());
        matched.insert("n2".to_string());

        let count = count_distinct_parent_modules(&matched, &nodes, &edges);
        assert_eq!(
            count, 2,
            "Should find 2 distinct parent modules via Contains edges"
        );
    }

    #[test]
    fn test_count_distinct_parent_modules_same_parent() {
        let nodes = vec![
            make_node("n1", "fn_a", NodeType::Function),
            make_node("n2", "fn_b", NodeType::Function),
            make_node("m1", "mod_a", NodeType::Module),
        ];
        let edges = vec![
            make_edge("e1", "m1", "n1", EdgeType::Contains),
            make_edge("e2", "m1", "n2", EdgeType::Contains),
        ];
        let mut matched = HashSet::new();
        matched.insert("n1".to_string());
        matched.insert("n2".to_string());

        let count = count_distinct_parent_modules(&matched, &nodes, &edges);
        assert_eq!(count, 1, "Both functions share the same parent module");
    }

    #[test]
    fn test_count_distinct_parent_modules_fallback_to_file_path() {
        let mut n1 = make_node("n1", "fn_a", NodeType::Function);
        n1.file_path = "src/auth/login.rs".to_string();
        let mut n2 = make_node("n2", "fn_b", NodeType::Function);
        n2.file_path = "src/auth/session.rs".to_string();
        let mut n3 = make_node("n3", "fn_c", NodeType::Function);
        n3.file_path = "src/db/query.rs".to_string();
        let nodes = vec![n1, n2, n3];

        // No Contains edges -- should fall back to file_path directory prefixes
        let edges: Vec<GraphEdge> = vec![];
        let mut matched = HashSet::new();
        matched.insert("n1".to_string());
        matched.insert("n2".to_string());
        matched.insert("n3".to_string());

        let count = count_distinct_parent_modules(&matched, &nodes, &edges);
        assert_eq!(count, 2, "Should find 2 distinct dirs: src/auth and src/db");
    }

    // ── $where(test_fragility, ...) tests ───────────────────────────────────

    #[test]
    fn test_where_test_fragility_filters_by_direct_test_callers() {
        let nodes = vec![
            make_node("n1", "well_tested", NodeType::Function),
            make_node("n2", "poorly_tested", NodeType::Function),
            make_node("n3", "untested", NodeType::Function),
            make_test_node("t1", "test_a"),
            make_test_node("t2", "test_b"),
        ];
        // t1 and t2 both call n1 (2 test callers); only t1 calls n2 (1 test caller)
        let edges = vec![
            make_edge("e1", "t1", "n1", EdgeType::Calls),
            make_edge("e2", "t2", "n1", EdgeType::Calls),
            make_edge("e3", "t1", "n2", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);

        // test_fragility > 1 should match n1 (2 callers) but not n2 (1) or n3 (0)
        let result = resolve_computed_expression(
            "$where(test_fragility, '>', 1)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.contains("n1"),
            "n1 has 2 test callers, should match > 1"
        );
        assert!(
            !result.contains("n2"),
            "n2 has 1 test caller, should not match > 1"
        );
        assert!(
            !result.contains("n3"),
            "n3 has 0 test callers, should not match > 1"
        );

        // test_fragility >= 1 should match n1 and n2
        let result2 = resolve_computed_expression(
            "$where(test_fragility, '>=', 1)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result2.contains("n1"));
        assert!(result2.contains("n2"));
        assert!(!result2.contains("n3"));
    }

    // ── Concept scope expand_direction tests ────────────────────────────────

    #[test]
    fn test_concept_outgoing_does_not_expand_bidirectionally() {
        // A -> B -> C and D -> A
        // With outgoing from A, we should get A, B, C but NOT D
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
            make_node("n4", "D", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
            make_edge("e3", "n4", "n1", EdgeType::Calls), // D calls A (incoming to A)
        ];
        let result = resolve_scope(
            &Scope::Concept {
                seed_nodes: vec!["A".to_string()],
                expand_edges: vec!["calls".to_string()],
                expand_depth: 5,
                expand_direction: "outgoing".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(result.contains("n1"), "Seed node A should be included");
        assert!(result.contains("n2"), "B is outgoing from A");
        assert!(result.contains("n3"), "C is outgoing from B");
        assert!(
            !result.contains("n4"),
            "D calls A but direction is outgoing, so D should NOT be included"
        );
    }

    #[test]
    fn test_concept_both_expands_bidirectionally() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n3", "n1", EdgeType::Calls), // C calls A (incoming to A)
        ];
        let result = resolve_scope(
            &Scope::Concept {
                seed_nodes: vec!["A".to_string()],
                expand_edges: vec!["calls".to_string()],
                expand_depth: 2,
                expand_direction: "both".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(result.contains("n1"), "Seed A");
        assert!(result.contains("n2"), "B via outgoing");
        assert!(result.contains("n3"), "C via incoming (both direction)");
    }

    #[test]
    fn test_concept_partial_seed_resolution_warns() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
        ];
        let edges = vec![make_edge("e1", "n1", "n2", EdgeType::Calls)];
        let result = resolve_scope_with_depths(
            &Scope::Concept {
                seed_nodes: vec!["A".to_string(), "B".to_string(), "NoSuchNode".to_string()],
                expand_edges: vec!["calls".to_string()],
                expand_depth: 2,
                expand_direction: "both".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        // Should contain per-node warning for the missing seed
        assert!(
            result.warnings.iter().any(|w| w.contains("NoSuchNode")),
            "Expected a warning about the missing seed node"
        );
        // Should also contain a summary partial-resolution warning
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("partially resolved") && w.contains("2 of 3")),
            "Expected a partial resolution warning, got: {:?}",
            result.warnings
        );
        // The resolved seeds should still produce results
        assert!(result.matched.contains("n1"));
        assert!(result.matched.contains("n2"));
    }

    // ── dry_run warns on computed expression errors ─────────────────────────

    #[test]
    fn test_dry_run_warns_on_invalid_computed_expression() {
        let nodes = vec![make_node("n1", "Foo", NodeType::Function)];
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec![],
                computed: Some("$bogus_expression".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Computed expression error")),
            "dry_run should include a warning about invalid computed expression, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_dry_run_no_warning_for_valid_computed() {
        let mut nodes = vec![make_node("n1", "complex_fn", NodeType::Function)];
        nodes[0].complexity = Some(30);
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec![],
                computed: Some("$where(complexity, '>', 10)".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.contains("Computed expression error")),
            "valid expression should not produce a computed expression error warning"
        );
        assert_eq!(result.matched_nodes, 1);
    }

    // ── $test_unreachable with Endpoints and Types ──────────────────────────

    #[test]
    fn test_computed_test_unreachable_includes_endpoints_and_types() {
        let nodes = vec![
            make_node("n1", "tested_fn", NodeType::Function),
            make_node("n2", "untested_endpoint", NodeType::Endpoint),
            make_node("n3", "untested_type", NodeType::Type),
            make_node("n4", "untested_module", NodeType::Module),
            make_node("n5", "untested_component", NodeType::Component),
            make_test_node("t1", "test_fn"),
        ];
        let edges = vec![make_edge("e1", "t1", "n1", EdgeType::Calls)];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$test_unreachable",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.contains("n2"),
            "Endpoints should be in $test_unreachable"
        );
        assert!(
            result.contains("n3"),
            "Types should be in $test_unreachable"
        );
        assert!(
            !result.contains("n4"),
            "Modules should NOT be in $test_unreachable"
        );
        assert!(
            !result.contains("n5"),
            "Components should NOT be in $test_unreachable"
        );
        assert!(
            !result.contains("n1"),
            "Tested functions should NOT be in $test_unreachable"
        );
    }

    // ── $intersect between $where and $test_unreachable ─────────────────────

    #[test]
    fn test_intersect_where_and_test_unreachable() {
        let mut nodes = vec![
            make_node("n1", "high_complex_untested", NodeType::Function),
            make_node("n2", "low_complex_untested", NodeType::Function),
            make_node("n3", "high_complex_tested", NodeType::Function),
            make_node("n4", "high_complex_untested_endpoint", NodeType::Endpoint),
            make_test_node("t1", "test_fn"),
        ];
        nodes[0].complexity = Some(50);
        nodes[1].complexity = Some(3);
        nodes[2].complexity = Some(40);
        nodes[3].complexity = Some(45);
        let edges = vec![make_edge("e1", "t1", "n3", EdgeType::Calls)];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$intersect($where(complexity, '>', 20), $test_unreachable)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(!result.contains("n2"));
        assert!(!result.contains("n3"));
        assert!(result.contains("n4"));
    }

    // ── $callers and $callees with depth limits ─────────────────────────────

    #[test]
    fn test_callers_with_depth_limit() {
        let nodes = vec![
            make_node("n1", "target", NodeType::Function),
            make_node("n2", "caller1", NodeType::Function),
            make_node("n3", "caller2", NodeType::Function),
            make_node("n4", "caller3", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n2", "n1", EdgeType::Calls),
            make_edge("e2", "n3", "n2", EdgeType::Calls),
            make_edge("e3", "n4", "n3", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);

        let result = resolve_computed_expression(
            "$callers(target, 1)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"), "target itself at depth 0");
        assert!(result.contains("n2"), "immediate caller at depth 1");
        assert!(
            !result.contains("n3"),
            "transitive caller at depth 2 excluded"
        );
        assert!(
            !result.contains("n4"),
            "transitive caller at depth 3 excluded"
        );

        let result2 = resolve_computed_expression(
            "$callers(target, 2)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result2.contains("n1"));
        assert!(result2.contains("n2"));
        assert!(result2.contains("n3"));
        assert!(!result2.contains("n4"));
    }

    #[test]
    fn test_callees_with_depth_limit() {
        let nodes = vec![
            make_node("n1", "root", NodeType::Function),
            make_node("n2", "callee1", NodeType::Function),
            make_node("n3", "callee2", NodeType::Function),
            make_node("n4", "callee3", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Calls),
            make_edge("e3", "n3", "n4", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);

        let result = resolve_computed_expression(
            "$callees(root, 2)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
        assert!(
            !result.contains("n4"),
            "depth 3 should be excluded with depth=2"
        );
    }

    // ── $governed_by with GovernedBy edges ──────────────────────────────────

    #[test]
    fn test_governed_by_multiple_code_nodes() {
        let mut nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "LoginHandler", NodeType::Function),
            make_node("n3", "auth_spec", NodeType::Type),
            make_node("n4", "Unrelated", NodeType::Function),
        ];
        nodes[2].spec_path = Some("specs/auth.md".to_string());
        let edges = vec![
            make_edge("e1", "n1", "n3", EdgeType::GovernedBy),
            make_edge("e2", "n2", "n3", EdgeType::GovernedBy),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$governed_by(auth.md)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"), "AuthService governed by auth spec");
        assert!(result.contains("n2"), "LoginHandler governed by auth spec");
        assert!(result.contains("n3"), "Spec node itself included");
        assert!(!result.contains("n4"), "Unrelated node excluded");
    }

    #[test]
    fn test_governed_by_fallback_without_edges() {
        let mut nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "other", NodeType::Function),
        ];
        nodes[0].spec_path = Some("specs/auth.md".to_string());
        let edges: Vec<GraphEdge> = vec![];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$governed_by(auth.md)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"), "Should fall back to spec_path match");
        assert!(!result.contains("n2"));
    }

    // ── Edge filtering in dry_run ───────────────────────────────────────────

    #[test]
    fn test_dry_run_edge_filtering_multiple_types() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Type),
            make_node("n3", "C", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n2", "n3", EdgeType::Contains),
            make_edge("e3", "n1", "n3", EdgeType::Implements),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: gyre_common::view_query::EdgeFilter {
                filter: vec!["calls".to_string(), "implements".to_string()],
            },
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert_eq!(result.matched_edges.len(), 2);
        let edge_types: Vec<&str> = result
            .matched_edges
            .iter()
            .map(|e| e.edge_type.as_str())
            .collect();
        assert!(edge_types.contains(&"calls"));
        assert!(edge_types.contains(&"implements"));
        assert!(!edge_types.contains(&"contains"));
    }

    #[test]
    fn test_dry_run_edge_filtering_empty_shows_all() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Function),
            make_node("n2", "B", NodeType::Function),
        ];
        let edges = vec![
            make_edge("e1", "n1", "n2", EdgeType::Calls),
            make_edge("e2", "n1", "n2", EdgeType::Contains),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: gyre_common::view_query::EdgeFilter { filter: vec![] },
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert_eq!(
            result.matched_edges.len(),
            2,
            "Empty filter should show all edge types"
        );
    }

    // ── Annotation template resolution ─────────────────────────────────────

    #[test]
    fn test_annotation_template_with_name_count_group() {
        let resolved = resolve_annotation_template(
            "$name: {{count}} nodes across {{group_count}} modules",
            Some("MyService"),
            25,
            4,
        );
        assert_eq!(resolved, "MyService: 25 nodes across 4 modules");
    }

    #[test]
    fn test_annotation_template_zero_counts() {
        let resolved = resolve_annotation_template(
            "Found {{count}} items in {{group_count}} groups",
            None,
            0,
            0,
        );
        assert_eq!(resolved, "Found 0 items in 0 groups");
    }

    // ── ViewQuery validation ───────────────────────────────────────────────

    #[test]
    fn test_viewquery_validate_depth_exceeds_max() {
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "A".to_string(),
                edges: vec!["calls".to_string()],
                direction: "outgoing".to_string(),
                depth: 150,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.iter().any(|e| e.contains("exceeds maximum")),
            "Should warn about depth > 100, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_concept_depth_exceeds_max() {
        let query = ViewQuery {
            scope: Scope::Concept {
                seed_nodes: vec!["A".to_string()],
                expand_edges: vec![],
                expand_depth: 200,
                expand_direction: "outgoing".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.iter().any(|e| e.contains("exceeds maximum")),
            "Should warn about expand_depth > 100, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_dim_unmatched_out_of_range() {
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: gyre_common::view_query::Emphasis {
                dim_unmatched: Some(1.5),
                ..Default::default()
            },
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("dim_unmatched") && e.contains("out of range")),
            "Should warn about dim_unmatched > 1.0, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_negative_dim_unmatched() {
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: gyre_common::view_query::Emphasis {
                dim_unmatched: Some(-0.1),
                ..Default::default()
            },
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.iter().any(|e| e.contains("dim_unmatched")),
            "Should warn about negative dim_unmatched, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_valid_passes() {
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "A".to_string(),
                edges: vec!["calls".to_string()],
                direction: "outgoing".to_string(),
                depth: 5,
            },
            emphasis: gyre_common::view_query::Emphasis {
                dim_unmatched: Some(0.3),
                ..Default::default()
            },
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.is_empty(),
            "Valid query should have no errors, got: {:?}",
            errors
        );
    }

    // ── Partial match warnings in dry_run ───────────────────────────────────

    #[test]
    fn test_dry_run_partial_match_warning() {
        let nodes = vec![make_node("n1", "AuthenticationService", NodeType::Type)];
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "Auth".to_string(),
                edges: vec![],
                direction: "outgoing".to_string(),
                depth: 2,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert!(
            result.warnings.iter().any(|w| w.contains("Partial match")),
            "Should warn about partial match, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_dry_run_no_partial_match_for_exact() {
        let nodes = vec![make_node("n1", "AuthService", NodeType::Type)];
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "AuthService".to_string(),
                edges: vec![],
                direction: "outgoing".to_string(),
                depth: 2,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert!(
            !result.warnings.iter().any(|w| w.contains("Partial match")),
            "Exact match should NOT produce partial match warning, got: {:?}",
            result.warnings
        );
    }

    // ── graph_summary computation ──────────────────────────────────────────

    #[test]
    fn test_graph_summary_top_types_by_fields() {
        let nodes = vec![
            make_node("t1", "BigType", NodeType::Type),
            make_node("t2", "SmallType", NodeType::Type),
            make_node("f1", "field_a", NodeType::Field),
            make_node("f2", "field_b", NodeType::Field),
            make_node("f3", "field_c", NodeType::Field),
            make_node("f4", "field_d", NodeType::Field),
        ];
        let edges = vec![
            make_edge("e1", "f1", "t1", EdgeType::FieldOf),
            make_edge("e2", "f2", "t1", EdgeType::FieldOf),
            make_edge("e3", "f3", "t1", EdgeType::FieldOf),
            make_edge("e4", "f4", "t2", EdgeType::FieldOf),
        ];
        let summary = compute_graph_summary("repo1", &nodes, &edges);
        assert!(
            !summary.top_types_by_fields.is_empty(),
            "Should have top types"
        );
        assert!(
            summary.top_types_by_fields[0].contains("BigType"),
            "BigType should be first, got: {:?}",
            summary.top_types_by_fields
        );
        assert!(
            summary.top_types_by_fields[0].contains("3 fields"),
            "Should show 3 fields, got: {}",
            summary.top_types_by_fields[0]
        );
    }

    #[test]
    fn test_graph_summary_test_coverage_counts() {
        let nodes = vec![
            make_node("n1", "fn_a", NodeType::Function),
            make_node("n2", "fn_b", NodeType::Function),
            make_node("n3", "fn_c", NodeType::Function),
            make_test_node("t1", "test_a"),
            make_test_node("t2", "test_b"),
        ];
        let edges = vec![
            make_edge("e1", "t1", "n1", EdgeType::Calls),
            make_edge("e2", "t2", "n2", EdgeType::Calls),
        ];
        let summary = compute_graph_summary("repo1", &nodes, &edges);
        assert_eq!(summary.test_coverage.test_functions, 2);
        // Reachable: BFS from t1 reaches {t1,n1}; from t2 reaches {t2,n2}. Total = 4.
        assert_eq!(summary.test_coverage.reachable_from_tests, 4);
        // Total functions = 5, unreachable = 1 (n3)
        assert_eq!(
            summary.test_coverage.unreachable, 1,
            "Only n3 should be unreachable"
        );
    }

    #[test]
    fn test_computed_where_risk_score() {
        // risk_score = churn × complexity × (1 - test_coverage)
        let mut n1 = make_node("n1", "risky_fn", NodeType::Function);
        n1.churn_count_30d = 10;
        n1.complexity = Some(20);
        n1.test_coverage = Some(0.0);
        // risk_score = 10 * 20 * 1.0 = 200

        let mut n2 = make_node("n2", "safe_fn", NodeType::Function);
        n2.churn_count_30d = 1;
        n2.complexity = Some(2);
        n2.test_coverage = Some(0.9);
        // risk_score = 1 * 2 * 0.1 = 0.2

        let nodes = vec![n1, n2];
        let edges: Vec<GraphEdge> = vec![];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression(
            "$where(risk_score, '>', 10)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.contains("n1"),
            "risky_fn should match risk_score > 10"
        );
        assert!(
            !result.contains("n2"),
            "safe_fn should not match risk_score > 10"
        );
    }

    #[test]
    fn test_find_node_by_name_deterministic() {
        let n1 = make_node("n1", "AuthService", NodeType::Type);
        let n2 = make_node("n2", "OAuthProvider", NodeType::Type);
        let n3 = make_node("n3", "AuthMiddleware", NodeType::Type);
        let nodes = vec![&n1, &n2, &n3];
        // Partial match for "Auth" should return the shortest name match
        let result = find_node_by_name(&nodes, "Auth");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "AuthService");
    }

    // ── Scope::Diff tests ─────────────────────────────────────────────

    #[test]
    fn test_scope_diff_temporal() {
        let mut n1 = make_node("n1", "OldType", NodeType::Type);
        n1.created_at = 1000;
        n1.last_modified_at = 1500;
        let mut n2 = make_node("n2", "NewType", NodeType::Type);
        n2.created_at = 2000;
        n2.last_modified_at = 2000;
        let mut n3 = make_node("n3", "AncientType", NodeType::Type);
        n3.created_at = 500;
        n3.last_modified_at = 500;

        let nodes = vec![n1, n2, n3];
        let edges = vec![];
        let scope = Scope::Diff {
            from_commit: "~1000".to_string(),
            to_commit: "~2000".to_string(),
        };
        let result = resolve_scope(&scope, &nodes, &edges, None);
        // n1 modified at 1500 (in range 1000-2000) and n2 created at 2000 (at boundary)
        assert!(result.contains("n1"), "n1 modified in range");
        assert!(result.contains("n2"), "n2 created in range");
        assert!(!result.contains("n3"), "n3 before range");
    }

    #[test]
    fn test_scope_diff_same_commit_warns() {
        let nodes = vec![make_node("n1", "Foo", NodeType::Type)];
        let edges = vec![];
        let scope = Scope::Diff {
            from_commit: "abc123".to_string(),
            to_commit: "abc123".to_string(),
        };
        let result = resolve_scope_with_depths(&scope, &nodes, &edges, None);
        assert!(
            result.warnings.iter().any(|w| w.contains("same commit")),
            "Should warn about identical commits"
        );
    }

    #[test]
    fn test_scope_diff_sha_prefix_matching() {
        let mut n1 = make_node("n1", "Changed", NodeType::Type);
        n1.last_modified_sha = "abcdef1234567890".to_string();
        n1.created_sha = "1111111111111111".to_string(); // different from from_commit
        let nodes = vec![n1];
        let edges = vec![];
        // 6-char SHA prefix should NOT match (minimum 7 required)
        let scope_short = Scope::Diff {
            from_commit: "0000000".to_string(),
            to_commit: "abcdef".to_string(),
        };
        let result_short = resolve_scope(&scope_short, &nodes, &edges, None);
        assert!(
            !result_short.contains("n1"),
            "6-char SHA prefix should not match (minimum 7 chars required)"
        );
        // 7-char SHA prefix should match via prefix
        let scope = Scope::Diff {
            from_commit: "0000000".to_string(),
            to_commit: "abcdef1".to_string(),
        };
        let result = resolve_scope(&scope, &nodes, &edges, None);
        assert!(
            result.contains("n1"),
            "7-char SHA prefix should match last_modified_sha"
        );
    }

    #[test]
    fn test_diff_scope_accepts_branch_names() {
        // Diff scope should accept branch names like "main", "HEAD", "v1.0"
        let q = ViewQuery {
            scope: Scope::Diff {
                from_commit: "main".to_string(),
                to_commit: "HEAD".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = q.validate();
        assert!(
            errors.is_empty(),
            "Branch names should be valid: {:?}",
            errors
        );

        let q2 = ViewQuery {
            scope: Scope::Diff {
                from_commit: "feature/my-branch".to_string(),
                to_commit: "v1.0.0".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors2 = q2.validate();
        assert!(
            errors2.is_empty(),
            "Feature branch names should be valid: {:?}",
            errors2
        );
    }

    // ── Unicode safety tests ──────────────────────────────────────────

    #[test]
    fn test_split_balanced_args_unicode() {
        // Multi-byte UTF-8 chars should not cause panics
        let result = split_balanced_args("$where(名前, =, 5), $callers(関数)");
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("名前"));
        assert!(result[1].contains("関数"));
    }

    #[test]
    fn test_find_balanced_comma_unicode() {
        let result = find_balanced_comma("$where(café, =, 5), other");
        assert!(result.is_some());
        // The comma should be after the closing paren
        let pos = result.unwrap();
        assert_eq!(&"$where(café, =, 5), other"[pos..pos + 1], ",");
    }

    // ── $where edge cases ──────────────────────────────────────────────

    #[test]
    fn test_where_unrecognized_property() {
        let n1 = make_node("n1", "Foo", NodeType::Type);
        let nodes = vec![n1];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let edges = vec![];
        let (outgoing, incoming) = build_adjacency(&edges);
        // Unknown property should return empty set
        let result = resolve_computed_expression(
            "$where(nonexistent_prop, >, 0)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.is_empty(),
            "Unrecognized property should match nothing"
        );
    }

    #[test]
    fn test_where_non_numeric_value() {
        let n1 = make_node("n1", "Foo", NodeType::Type);
        let nodes = vec![n1];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let edges = vec![];
        let (outgoing, incoming) = build_adjacency(&edges);
        // Non-numeric value should return empty set
        let result = resolve_computed_expression(
            "$where(complexity, >, not_a_number)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.is_empty(),
            "Non-numeric value should return empty set"
        );
    }

    // ── parse_edge_type / parse_node_type tests ────────────────────────

    #[test]
    fn test_parse_edge_type_valid() {
        assert_eq!(parse_edge_type("calls"), Some(EdgeType::Calls));
        assert_eq!(parse_edge_type("Calls"), Some(EdgeType::Calls));
        assert_eq!(parse_edge_type("CALLS"), Some(EdgeType::Calls));
        assert_eq!(parse_edge_type("depends_on"), Some(EdgeType::DependsOn));
        assert_eq!(parse_edge_type("dependson"), Some(EdgeType::DependsOn));
        assert_eq!(parse_edge_type("governed_by"), Some(EdgeType::GovernedBy));
    }

    #[test]
    fn test_parse_edge_type_invalid() {
        assert_eq!(parse_edge_type("invalid_edge"), None);
        assert_eq!(parse_edge_type(""), None);
        assert_eq!(parse_edge_type("call"), None); // typo — no 's'
    }

    #[test]
    fn test_parse_node_type_valid() {
        assert_eq!(parse_node_type("type"), Some(NodeType::Type));
        assert_eq!(parse_node_type("Type"), Some(NodeType::Type));
        assert_eq!(parse_node_type("function"), Some(NodeType::Function));
        assert_eq!(parse_node_type("endpoint"), Some(NodeType::Endpoint));
        assert_eq!(parse_node_type("spec"), Some(NodeType::Spec));
    }

    #[test]
    fn test_parse_node_type_invalid() {
        assert_eq!(parse_node_type("clase"), None); // typo
        assert_eq!(parse_node_type(""), None);
        assert_eq!(parse_node_type("widget"), None); // not a valid node type name
    }

    #[test]
    fn test_parse_node_type_new_variants() {
        assert_eq!(parse_node_type("trait"), Some(NodeType::Trait));
        assert_eq!(parse_node_type("method"), Some(NodeType::Method));
        assert_eq!(parse_node_type("class"), Some(NodeType::Class));
        assert_eq!(parse_node_type("enum"), Some(NodeType::Enum));
        assert_eq!(parse_node_type("enum_variant"), Some(NodeType::EnumVariant));
        assert_eq!(parse_node_type("variant"), Some(NodeType::EnumVariant));
        assert_eq!(parse_node_type("struct"), Some(NodeType::Type)); // alias
    }

    // ── normalize_computed_expression tests ─────────────────────────────

    #[test]
    fn test_normalize_spaces_before_paren() {
        assert_eq!(
            normalize_computed_expression("$callers (Foo)"),
            "$callers(Foo)"
        );
        assert_eq!(
            normalize_computed_expression("$callers  (Foo, 5)"),
            "$callers(Foo, 5)"
        );
    }

    #[test]
    fn test_normalize_preserves_inner_content() {
        assert_eq!(
            normalize_computed_expression("$where(complexity, '>', 20)"),
            "$where(complexity, '>', 20)"
        );
    }

    #[test]
    fn test_normalize_no_paren() {
        assert_eq!(
            normalize_computed_expression("$test_unreachable"),
            "$test_unreachable"
        );
    }

    #[test]
    fn test_normalize_tabs_and_spaces() {
        assert_eq!(
            normalize_computed_expression("$callers\t(Foo)"),
            "$callers(Foo)" // all ASCII whitespace before paren is normalized
        );
    }

    // ── validate_computed_expression tests ──────────────────────────────

    #[test]
    fn test_validate_typo_expression() {
        // $caller (missing 's') should be flagged as unrecognized
        let result = validate_computed_expression("$caller(Foo)");
        assert!(result.is_some(), "Typo expression should be invalid");
        assert!(
            result.unwrap().contains("Unrecognized"),
            "Should say unrecognized"
        );
    }

    #[test]
    fn test_validate_valid_expressions() {
        assert!(validate_computed_expression("$test_unreachable").is_none());
        assert!(validate_computed_expression("$callers(Foo)").is_none());
        assert!(validate_computed_expression("$where(complexity, '>', 20)").is_none());
        assert!(validate_computed_expression(
            "$intersect($test_unreachable, $where(complexity, '>', 5))"
        )
        .is_none());
    }

    // ── Computed expression filter warnings in dry_run ──────────────────

    #[test]
    fn test_filter_scope_warns_on_unrecognized_node_type() {
        let nodes = vec![make_node("n1", "Foo", NodeType::Type)];
        let edges = vec![];
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec!["typo_type".to_string()],
                computed: None,
                name_pattern: None,
            },
            ..ViewQuery {
                scope: Scope::All,
                emphasis: Default::default(),
                edges: Default::default(),
                zoom: Default::default(),
                annotation: Default::default(),
                groups: vec![],
                callouts: vec![],
                narrative: vec![],
            }
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Unrecognized node type")),
            "Should warn about unrecognized node type: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_edge_filter_warns_on_unrecognized_edge_type() {
        let nodes = vec![
            make_node("n1", "A", NodeType::Type),
            make_node("n2", "B", NodeType::Type),
        ];
        let edges = vec![make_edge("e1", "n1", "n2", EdgeType::Calls)];
        let mut query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        query.edges.filter = vec!["typo_edge".to_string()];
        let result = dry_run(&query, &nodes, &edges, None);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Unrecognized edge type")),
            "Should warn about unrecognized edge type: {:?}",
            result.warnings
        );
    }

    // ── Risk score consistency test ──────────────────────────────────────

    #[test]
    fn test_risk_score_no_complexity_excluded() {
        let mut n1 = make_node("n1", "HasComplexity", NodeType::Function);
        n1.complexity = Some(10);
        n1.churn_count_30d = 5;
        n1.test_coverage = Some(0.5);
        let mut n2 = make_node("n2", "NoComplexity", NodeType::Function);
        n2.complexity = None;
        n2.churn_count_30d = 5;
        n2.test_coverage = Some(0.5);

        let nodes = vec![n1, n2];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let edges = vec![];
        let (outgoing, incoming) = build_adjacency(&edges);

        // $where(risk_score, >, 0) should only include nodes with complexity data
        let result = resolve_computed_expression(
            "$where(risk_score, >, 0)",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(
            result.contains("n1"),
            "Node with complexity should be included"
        );
        assert!(
            !result.contains("n2"),
            "Node without complexity should be excluded"
        );

        // Heat emphasis should also exclude nodes without complexity
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: gyre_common::view_query::Emphasis {
                heat: Some(gyre_common::view_query::HeatConfig {
                    metric: "risk_score".to_string(),
                    palette: "blue-red".to_string(),
                }),
                ..Default::default()
            },
            ..ViewQuery {
                scope: Scope::All,
                emphasis: Default::default(),
                edges: Default::default(),
                zoom: Default::default(),
                annotation: Default::default(),
                groups: vec![],
                callouts: vec![],
                narrative: vec![],
            }
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert!(
            result.node_metrics.contains_key("n1"),
            "n1 should have risk metric"
        );
        assert!(
            !result.node_metrics.contains_key("n2"),
            "n2 should not have risk metric"
        );
    }

    // ── Scope::Diff tests ─────────────────────────────────────────────────

    #[test]
    fn test_diff_scope_temporal() {
        let mut n1 = make_node("n1", "A", NodeType::Function);
        n1.created_at = 1000;
        n1.last_modified_at = 1200;
        let mut n2 = make_node("n2", "B", NodeType::Function);
        n2.created_at = 1500;
        n2.last_modified_at = 1600;
        let mut n3 = make_node("n3", "C", NodeType::Function);
        n3.created_at = 2000;
        n3.last_modified_at = 2100;
        let nodes = vec![n1, n2, n3];
        let edges: Vec<GraphEdge> = vec![];
        let result = resolve_scope(
            &Scope::Diff {
                from_commit: "~1400".to_string(),
                to_commit: "~1700".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        // B was created at 1500 (within 1400..1700)
        assert!(result.contains("n2"), "B should be in diff range");
        // A was created at 1000 (before range), modified at 1200 (before range)
        assert!(!result.contains("n1"), "A should not be in range");
        // C was created at 2000 (after range)
        assert!(!result.contains("n3"), "C should not be in range");
    }

    #[test]
    fn test_diff_scope_same_commit_warning() {
        let nodes = vec![make_node("n1", "A", NodeType::Function)];
        let edges: Vec<GraphEdge> = vec![];
        let query = ViewQuery {
            scope: Scope::Diff {
                from_commit: "abcd1234".to_string(),
                to_commit: "abcd1234".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert!(
            result.warnings.iter().any(|w| w.contains("same commit")),
            "Should warn when from and to are identical"
        );
    }

    #[test]
    fn test_diff_scope_sha_requires_min_7_chars() {
        let mut node = make_node("n1", "A", NodeType::Function);
        node.last_modified_sha = "abcdef1234567890".to_string();
        node.created_sha = "1111111111111111".to_string();
        let nodes = vec![node];
        let edges: Vec<GraphEdge> = vec![];
        // Short SHA "ab" (2 chars) should not match
        let result = resolve_scope(
            &Scope::Diff {
                from_commit: "0000000".to_string(),
                to_commit: "ab".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(
            !result.contains("n1"),
            "2-char SHA should not match due to min 7-char requirement"
        );
        // 6-char SHA prefix should NOT match (minimum 7 required)
        let result2 = resolve_scope(
            &Scope::Diff {
                from_commit: "0000000".to_string(),
                to_commit: "abcdef".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(
            !result2.contains("n1"),
            "6-char SHA prefix should not match (minimum 7 chars required)"
        );
        // 7-char SHA prefix should match
        let result3 = resolve_scope(
            &Scope::Diff {
                from_commit: "0000000".to_string(),
                to_commit: "abcdef1".to_string(),
            },
            &nodes,
            &edges,
            None,
        );
        assert!(result3.contains("n1"), "7-char SHA prefix should match");
    }

    // ── Filter scope: computed + node_types combined ─────────────────────

    #[test]
    fn test_filter_scope_computed_and_node_types_combined() {
        // When a Filter scope has BOTH computed and node_types, both should apply.
        // Previously computed took priority and node_types was silently ignored.
        let mut n1 = make_node("n1", "HighComplexFunc", NodeType::Function);
        n1.complexity = Some(10);
        let mut n2 = make_node("n2", "HighComplexType", NodeType::Type);
        n2.complexity = Some(15);
        let mut n3 = make_node("n3", "LowComplexFunc", NodeType::Function);
        n3.complexity = Some(2);
        let mut n4 = make_node("n4", "HighComplexModule", NodeType::Module);
        n4.complexity = Some(20);

        let nodes = vec![n1, n2, n3, n4];
        let edges: Vec<GraphEdge> = vec![];
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec!["function".to_string(), "type".to_string()],
                computed: Some("$where(complexity, '>', 5)".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        // matched_node_names uses qualified_name (pkg.Name format from make_node)
        assert_eq!(
            result.matched_nodes, 2,
            "Should match 2 nodes (function + type with complexity > 5)"
        );
        assert!(
            result
                .matched_node_names
                .iter()
                .any(|n| n.contains("HighComplexFunc")),
            "Function with high complexity should be included, got: {:?}",
            result.matched_node_names
        );
        assert!(
            result
                .matched_node_names
                .iter()
                .any(|n| n.contains("HighComplexType")),
            "Type with high complexity should be included"
        );
        assert!(
            !result
                .matched_node_names
                .iter()
                .any(|n| n.contains("LowComplexFunc")),
            "Low complexity function should be excluded"
        );
        assert!(
            !result
                .matched_node_names
                .iter()
                .any(|n| n.contains("HighComplexModule")),
            "Module should be excluded because node_types filter says only function+type"
        );
    }

    #[test]
    fn test_filter_scope_computed_only_no_node_types_includes_all_types() {
        // When computed is set but node_types is empty, all node types should pass
        let mut n1 = make_node("n1", "Func", NodeType::Function);
        n1.complexity = Some(10);
        let mut n2 = make_node("n2", "Mod", NodeType::Module);
        n2.complexity = Some(10);

        let nodes = vec![n1, n2];
        let edges: Vec<GraphEdge> = vec![];
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec![],
                computed: Some("$where(complexity, '>', 5)".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &edges, None);
        assert_eq!(
            result.matched_nodes, 2,
            "Both nodes should match when no node_types filter"
        );
    }

    #[test]
    fn test_where_node_type_string() {
        // Test that $where(node_type, '=', 'function') works for string properties
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "bar_fn", NodeType::Function),
            make_node("n3", "Baz", NodeType::Type),
            make_node("n4", "baz_fn", NodeType::Function),
        ];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: Some("$where(node_type, '=', 'function')".to_string()),
                name_pattern: None,
            },
            &nodes,
            &[],
            None,
        );
        assert_eq!(result.len(), 2, "Should match only function nodes");
        assert!(result.contains("n2"));
        assert!(result.contains("n4"));
    }

    #[test]
    fn test_where_node_type_not_equal() {
        let nodes = vec![
            make_node("n1", "Foo", NodeType::Type),
            make_node("n2", "bar_fn", NodeType::Function),
        ];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: Some("$where(node_type, '!=', 'function')".to_string()),
                name_pattern: None,
            },
            &nodes,
            &[],
            None,
        );
        assert_eq!(result.len(), 1);
        assert!(result.contains("n1"));
    }

    #[test]
    fn test_group_exact_match_preferred() {
        // Group resolution should prefer exact match over substring
        let nodes = vec![
            make_node("n1", "Task", NodeType::Type),
            make_node("n2", "TaskPort", NodeType::Trait),
            make_node("n3", "TaskFilter", NodeType::Type),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![gyre_common::view_query::ViewGroup {
                name: "core".to_string(),
                nodes: vec!["Task".to_string()],
                color: None,
                label: None,
            }],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        // Should match exactly "Task", not "TaskPort" or "TaskFilter"
        assert_eq!(
            result.groups_resolved[0].matched, 1,
            "Exact match should find only Task"
        );
        assert!(result.groups_resolved[0]
            .nodes
            .contains(&"pkg.Task".to_string()));
    }

    #[test]
    fn test_group_substring_fallback() {
        // When no exact match, substring matching should work as fallback
        let nodes = vec![
            make_node("n1", "AuthHandler", NodeType::Function),
            make_node("n2", "AuthMiddleware", NodeType::Function),
            make_node("n3", "UserService", NodeType::Type),
        ];
        let query = ViewQuery {
            scope: Scope::All,
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![gyre_common::view_query::ViewGroup {
                name: "auth".to_string(),
                nodes: vec!["Auth".to_string()],
                color: None,
                label: None,
            }],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        // No exact match for "Auth", so substring should match both
        assert_eq!(result.groups_resolved[0].matched, 2);
    }

    #[test]
    fn test_clicked_no_selection_warning() {
        let nodes = vec![make_node("n1", "Foo", NodeType::Type)];
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "$clicked".to_string(),
                edges: vec![],
                direction: "outgoing".to_string(),
                depth: 5,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let result = dry_run(&query, &nodes, &[], None);
        assert_eq!(result.matched_nodes, 0);
        assert!(
            result.warnings.iter().any(|w| w.contains("click a node")),
            "Should show helpful message when $clicked used with no selection"
        );
    }

    #[test]
    fn test_diff_scope_validation() {
        use gyre_common::view_query::ViewQuery;
        // Empty from_commit
        let q = ViewQuery {
            scope: Scope::Diff {
                from_commit: "".to_string(),
                to_commit: "abcd1234".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errs = q.validate();
        assert!(errs.iter().any(|e| e.contains("must not be empty")));

        // Short SHA
        let q2 = ViewQuery {
            scope: Scope::Diff {
                from_commit: "ab".to_string(),
                to_commit: "abcd1234".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errs2 = q2.validate();
        assert!(errs2.iter().any(|e| e.contains("too short")));

        // Same commit
        let q3 = ViewQuery {
            scope: Scope::Diff {
                from_commit: "abcd1234".to_string(),
                to_commit: "abcd1234".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errs3 = q3.validate();
        assert!(errs3.iter().any(|e| e.contains("identical")));
    }

    #[test]
    fn test_resolver_max_depth_matches_validation() {
        // Ensure nested expressions up to depth 10 work (both validate and resolve)
        let deep_expr = "$intersect($union($test_unreachable, $test_reachable), $test_unreachable)";
        let nodes = vec![
            make_node("n1", "foo", NodeType::Function),
            make_test_node("t1", "test_foo"),
        ];
        let edges = vec![make_edge("e1", "t1", "n1", EdgeType::Calls)];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: Some(deep_expr.to_string()),
                name_pattern: None,
            },
            &nodes,
            &edges,
            None,
        );
        // $test_unreachable ∩ ($test_unreachable ∪ $test_reachable) = $test_unreachable
        // But n1 IS reachable via the calls edge, so unreachable set is empty
        // (test_foo reaches foo via Calls)
        assert_eq!(result.len(), 0, "All functions are reachable from tests");
    }

    #[test]
    fn test_callers_default_depth_is_5() {
        // Verify default depth for $callers is 5, not RESOLVER_MAX_DEPTH (20)
        let mut nodes = vec![make_node("n0", "root", NodeType::Function)];
        let mut edges = vec![];
        // Create a chain: n0 <- n1 <- n2 <- n3 <- n4 <- n5 <- n6
        for i in 1..=6 {
            nodes.push(make_node(
                &format!("n{}", i),
                &format!("fn_{}", i),
                NodeType::Function,
            ));
            edges.push(make_edge(
                &format!("e{}", i),
                &format!("n{}", i),
                &format!("n{}", i - 1),
                EdgeType::Calls,
            ));
        }
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: Some("$callers(root)".to_string()),
                name_pattern: None,
            },
            &nodes,
            &edges,
            None,
        );
        // Default depth 5: should include n0 (root) + n1..n5 = 6 nodes
        // n6 is at depth 6, should be excluded
        assert_eq!(
            result.len(),
            6,
            "Default $callers depth should be 5 (root + 5 hops)"
        );
        assert!(
            !result.contains("n6"),
            "n6 is at depth 6, should be excluded"
        );
    }

    // ── $ungoverned ──────────────────────────────────────────────────────

    #[test]
    fn test_ungoverned_returns_nodes_without_spec() {
        let mut nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "LoginHandler", NodeType::Function),
            make_node("n3", "Unspecified", NodeType::Type),
        ];
        // n1 has GovernedBy edge
        nodes[1].spec_path = Some("specs/auth.md".to_string());
        let edges = vec![make_edge("e1", "n1", "spec_node", EdgeType::GovernedBy)];
        let (outgoing, incoming) = build_adjacency(&edges);
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let result =
            resolve_computed_expression("$ungoverned", &active, &edges, &outgoing, &incoming, None);
        // n1 is governed by edge, n2 has spec_path, n3 has neither
        assert!(result.contains("n3"), "n3 has no governance");
        assert!(!result.contains("n1"), "n1 has GovernedBy edge");
        assert!(!result.contains("n2"), "n2 has spec_path");
    }

    #[test]
    fn test_ungoverned_excludes_spec_and_module_nodes() {
        let mut spec_node = make_node("s1", "my_spec", NodeType::Spec);
        spec_node.spec_path = None; // Spec node without spec_path still excluded
        let mod_node = make_node("m1", "my_module", NodeType::Module);
        let func_node = make_node("f1", "orphan_func", NodeType::Function);
        let nodes = vec![spec_node, mod_node, func_node];
        let edges: Vec<GraphEdge> = vec![];
        let (outgoing, incoming) = build_adjacency(&edges);
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let result =
            resolve_computed_expression("$ungoverned", &active, &edges, &outgoing, &incoming, None);
        assert!(!result.contains("s1"), "Spec nodes excluded");
        assert!(!result.contains("m1"), "Module nodes excluded");
        assert!(result.contains("f1"), "Ungoverned function included");
    }

    #[test]
    fn test_ungoverned_intersect_with_where() {
        let mut nodes = vec![
            make_node("n1", "SimpleFunc", NodeType::Function),
            make_node("n2", "ComplexFunc", NodeType::Function),
        ];
        nodes[0].complexity = Some(5);
        nodes[1].complexity = Some(15);
        let edges: Vec<GraphEdge> = vec![];
        let result = resolve_scope(
            &Scope::Filter {
                node_types: vec![],
                computed: Some("$intersect($where(complexity, '>', 10), $ungoverned)".to_string()),
                name_pattern: None,
            },
            &nodes,
            &edges,
            None,
        );
        assert_eq!(result.len(), 1, "Only complex ungoverned func");
        assert!(result.contains("n2"), "ComplexFunc has complexity 15 > 10");
        assert!(!result.contains("n1"), "SimpleFunc has complexity 5 <= 10");
    }

    #[test]
    fn test_validate_ungoverned_expression() {
        assert!(validate_computed_expression("$ungoverned").is_none());
        assert!(validate_computed_expression(
            "$intersect($where(complexity, '>', 10), $ungoverned)"
        )
        .is_none());
    }

    #[test]
    fn test_validate_standalone_keywords_exact_match() {
        // Standalone keywords must match exactly, not as prefixes
        assert!(
            validate_computed_expression("$ungoverned_extra").is_some(),
            "$ungoverned_extra should be rejected"
        );
        assert!(
            validate_computed_expression("$clicked_thing").is_some(),
            "$clicked_thing should be rejected"
        );
        assert!(
            validate_computed_expression("$selected_nodes").is_some(),
            "$selected_nodes should be rejected"
        );
        assert!(
            validate_computed_expression("$test_unreachable_stuff").is_some(),
            "$test_unreachable_stuff should be rejected"
        );
        // But valid standalone keywords pass
        assert!(validate_computed_expression("$clicked").is_none());
        assert!(validate_computed_expression("$selected").is_none());
        assert!(validate_computed_expression("$test_unreachable").is_none());
        assert!(validate_computed_expression("$test_reachable").is_none());
    }

    #[test]
    fn test_governed_by_exact_matching() {
        // Exact path matching: $governed_by('auth.md') should NOT match 'authorization.md'
        let mut nodes = vec![
            make_node("n1", "AuthService", NodeType::Type),
            make_node("n2", "AuthzService", NodeType::Type),
            make_node("n3", "auth_spec", NodeType::Spec),
            make_node("n4", "authz_spec", NodeType::Spec),
        ];
        nodes[2].spec_path = Some("specs/system/auth.md".to_string());
        nodes[3].spec_path = Some("specs/system/authorization.md".to_string());
        let edges = vec![
            make_edge("e1", "n1", "n3", EdgeType::GovernedBy),
            make_edge("e2", "n2", "n4", EdgeType::GovernedBy),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);

        // Query for 'auth.md' should only match the auth spec, not authorization
        let result = resolve_computed_expression(
            "$governed_by('auth.md')",
            &active,
            &edges,
            &outgoing,
            &incoming,
            None,
        );
        assert!(result.contains("n1"), "AuthService governed by auth.md");
        assert!(result.contains("n3"), "auth spec node itself");
        assert!(
            !result.contains("n2"),
            "AuthzService should NOT match auth.md (exact matching)"
        );
        assert!(
            !result.contains("n4"),
            "authz spec should NOT match auth.md (exact matching)"
        );
    }

    #[test]
    fn test_viewquery_validate_focus_invalid_direction() {
        let query = ViewQuery {
            scope: Scope::Focus {
                node: "A".to_string(),
                edges: vec![],
                direction: "sideways".to_string(),
                depth: 5,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.iter().any(|e| e.contains("Invalid direction")),
            "Should reject invalid Focus direction, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_concept_invalid_direction() {
        let query = ViewQuery {
            scope: Scope::Concept {
                seed_nodes: vec!["A".to_string()],
                expand_edges: vec![],
                expand_depth: 2,
                expand_direction: "diagonal".to_string(),
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Invalid expand_direction")),
            "Should reject invalid Concept expand_direction, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_where_unknown_node_type() {
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec![],
                computed: Some("$where(node_type, '=', 'func')".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.iter().any(|e| e.contains("not recognized")),
            "Should warn about unknown node_type value 'func', got: {:?}",
            errors
        );
    }

    #[test]
    fn test_viewquery_validate_where_valid_node_type() {
        let query = ViewQuery {
            scope: Scope::Filter {
                node_types: vec![],
                computed: Some("$where(node_type, '=', 'function')".to_string()),
                name_pattern: None,
            },
            emphasis: Default::default(),
            edges: Default::default(),
            zoom: Default::default(),
            annotation: Default::default(),
            groups: vec![],
            callouts: vec![],
            narrative: vec![],
        };
        let errors = query.validate();
        assert!(
            errors.is_empty(),
            "Valid node_type='function' should pass, got: {:?}",
            errors
        );
    }
}
