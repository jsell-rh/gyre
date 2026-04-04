//! View Query Resolver — resolves view queries against the knowledge graph.
//!
//! This is the core engine that both the dry-run MCP tool and the frontend renderer use.
//! Given a ViewQuery and a set of nodes/edges, produces a resolved result set with
//! matched nodes, groups, callouts, narrative steps, and warnings.

use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType};
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
        "type" => Some(NodeType::Type),
        "interface" => Some(NodeType::Interface),
        "function" => Some(NodeType::Function),
        "endpoint" => Some(NodeType::Endpoint),
        "component" => Some(NodeType::Component),
        "table" => Some(NodeType::Table),
        "constant" => Some(NodeType::Constant),
        "field" => Some(NodeType::Field),
        "spec" => Some(NodeType::Spec),
        _ => None,
    }
}

fn node_type_str(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Package => "package",
        NodeType::Module => "module",
        NodeType::Type => "type",
        NodeType::Interface => "interface",
        NodeType::Function => "function",
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
/// Deterministic: exact match first, then shortest partial match with alphabetical tiebreak.
fn find_node_by_name_with_match_type<'a>(
    nodes: &[&'a GraphNode],
    name: &str,
) -> Option<(&'a GraphNode, bool)> {
    let lower = name.to_lowercase();
    // Exact match first (qualified_name then name).
    if let Some(exact) = nodes
        .iter()
        .find(|n| n.qualified_name.to_lowercase() == lower || n.name.to_lowercase() == lower)
    {
        return Some((exact, false));
    }
    // Partial match fallback: pick shortest name match (most specific),
    // with alphabetical tiebreak for determinism.
    let mut candidates: Vec<&'a GraphNode> = nodes
        .iter()
        .filter(|n| {
            n.qualified_name.to_lowercase().contains(&lower)
                || n.name.to_lowercase().contains(&lower)
        })
        .copied()
        .collect();
    candidates.sort_by(|a, b| {
        a.name
            .len()
            .cmp(&b.name.len())
            .then_with(|| a.qualified_name.cmp(&b.qualified_name))
    });
    candidates.first().map(|n| (*n, true))
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
                None => (HashSet::new(), no_depths()),
            }
        }

        Scope::Filter {
            node_types,
            computed,
            name_pattern,
        } => {
            if let Some(expr) = computed {
                let result = resolve_computed_expression(
                    expr,
                    &active_nodes,
                    edges,
                    outgoing,
                    incoming,
                    selected_node_id,
                );
                (result, no_depths())
            } else {
                let types: Vec<NodeType> = node_types
                    .iter()
                    .filter_map(|s| parse_node_type(s))
                    .collect();
                let pattern = name_pattern.as_ref().map(|p| p.to_lowercase());
                let result = active_nodes
                    .iter()
                    .filter(|n| {
                        let type_match = types.is_empty() || types.contains(&n.node_type);
                        let name_match = pattern.as_ref().map_or(true, |p| {
                            n.name.to_lowercase().contains(p)
                                || n.qualified_name.to_lowercase().contains(p)
                        });
                        type_match && name_match
                    })
                    .map(|n| n.id.to_string())
                    .collect();
                (result, no_depths())
            }
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
                        NodeType::Function | NodeType::Endpoint | NodeType::Type
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
                // SHA-based diff: select nodes modified at or after to_commit but not
                // at from_commit. This is a best-effort heuristic — we only have the
                // last_modified_sha on each node, not the full git log, so nodes
                // changed by intermediate commits between from and to will be missed.
                // Prefer temporal diff (epoch prefix) for accurate ranges.
                warnings.push(
                    "SHA-based diff is approximate: only nodes whose last_modified_sha \
                     matches the target commit are shown. Intermediate commits are not \
                     visible. Use temporal diff (~epoch) for full range accuracy."
                        .to_string(),
                );
                let sha_matches = |sha: &str, target: &str| -> bool {
                    if target.is_empty() {
                        return false;
                    }
                    let sha_lower = sha.to_lowercase();
                    if target.len() >= 7 {
                        sha_lower.starts_with(target)
                    } else {
                        sha_lower == *target
                    }
                };

                active_nodes
                    .iter()
                    .filter(|n| {
                        let created_matches_to = sha_matches(&n.created_sha, &to_lower);
                        let modified_matches_to = sha_matches(&n.last_modified_sha, &to_lower);
                        let created_matches_from = sha_matches(&n.created_sha, &from_lower);

                        // Node was created or modified at to_commit, but not created at from_commit
                        (created_matches_to || modified_matches_to) && !created_matches_from
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
            let direction = if expand_direction.is_empty() {
                "outgoing"
            } else {
                expand_direction.as_str()
            };
            let mut result = HashSet::new();
            let mut all_depths = HashMap::new();

            for seed_name in seed_nodes {
                if let Some(seed) = find_node_by_name(&active_nodes, seed_name) {
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
            // Skip whitespace before opening paren
            while i < chars.len() && chars[i] == ' ' {
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
fn resolve_computed_expression(
    expr: &str,
    active_nodes: &[&GraphNode],
    edges: &[GraphEdge],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    incoming: &HashMap<String, Vec<(String, EdgeType)>>,
    selected_node_id: Option<&str>,
) -> HashSet<String> {
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
                    NodeType::Function | NodeType::Endpoint | NodeType::Type
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
            let val: f64 = match parts[2].parse() {
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
                            let churn = n.churn_count_30d as f64;
                            let complexity = n.complexity.unwrap_or(1) as f64;
                            let test_gap = 1.0 - n.test_coverage.unwrap_or(0.0);
                            Some(churn * complexity * test_gap)
                        }
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
            let set_a = resolve_computed_expression(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
            );
            let set_b = resolve_computed_expression(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
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
            let set_a = resolve_computed_expression(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
            );
            let set_b = resolve_computed_expression(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
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
            let set_a = resolve_computed_expression(
                a_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
            );
            let set_b = resolve_computed_expression(
                b_expr,
                active_nodes,
                edges,
                outgoing,
                incoming,
                selected_node_id,
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

    // $descendants(node) — cap depth at 20 to prevent DoS from unbounded traversal
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
                20,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $ancestors(node) — cap depth at 20 to prevent DoS from unbounded traversal
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
                20,
                outgoing,
                incoming,
            );
        }
        return HashSet::new();
    }

    // $governed_by(spec_path) — use GovernedBy edge traversal, not spec_path substring
    if trimmed.starts_with("$governed_by(") && trimmed.ends_with(')') {
        let spec_path = trimmed[13..trimmed.len() - 1]
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        let lower = spec_path.to_lowercase();

        // First find spec nodes (or any node whose spec_path matches)
        let spec_node_ids: HashSet<String> = active_nodes
            .iter()
            .filter(|n| {
                n.spec_path
                    .as_ref()
                    .map_or(false, |sp| sp.to_lowercase().contains(&lower))
            })
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

        // Also include nodes that have a matching spec_path directly
        // (for backwards compatibility when GovernedBy edges aren't present)
        if governed.is_empty() {
            return spec_node_ids;
        }

        // Include the spec nodes themselves too
        governed.extend(spec_node_ids);
        return governed;
    }

    // $test_fragility(node) — returns the count of distinct test paths as a metric,
    // but for set operations returns the node if count > 0.
    // Uses batch computation for consistency with $where(test_fragility, ...).
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

    // $reachable(node, edge_types, direction, depth) — use find_balanced_comma for parsing
    if trimmed.starts_with("$reachable(") && trimmed.ends_with(')') {
        let inner = &trimmed[11..trimmed.len() - 1];
        // Parse using balanced comma finding for proper array handling
        let parts = split_balanced_args(inner);
        if parts.len() >= 2 {
            let node_name = parts[0].trim().trim_matches('\'').trim_matches('"');
            let edge_types_str = parts.get(1).unwrap_or(&"");
            let direction = parts
                .get(2)
                .map(|s| s.trim().trim_matches('\'').trim_matches('"'))
                .unwrap_or("outgoing");
            let depth: u32 = parts
                .get(3)
                .and_then(|d| d.trim().parse().ok())
                .unwrap_or(10);

            let edge_types: Vec<EdgeType> = edge_types_str
                .trim()
                .trim_matches(|c: char| c == '[' || c == ']')
                .split(',')
                .filter_map(|s| parse_edge_type(s.trim().trim_matches('\'').trim_matches('"')))
                .collect();

            let resolved_name = resolve_node_ref(node_name, selected_node_id);
            if let Some(found) = find_node_by_ref(active_nodes, &resolved_name) {
                return bfs_traverse(
                    &found.id.to_string(),
                    &edge_types,
                    direction,
                    depth,
                    outgoing,
                    incoming,
                );
            }
        }
        return HashSet::new();
    }

    // Fallback: unrecognized expression — returns empty set.
    // validate_computed_expression() should be called before resolution to catch these.
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

    if !known_prefixes.iter().any(|p| trimmed.starts_with(p)) {
        return Some(format!(
            "Unrecognized expression: '{trimmed}'. Known: $where, $callers, $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $test_fragility, $reachable, $intersect, $union, $diff, $test_unreachable, $test_reachable, $clicked, $selected"
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
    // No depth separator found — entire thing is the node ref
    let node_ref = inner.trim_matches('\'').trim_matches('"');
    (node_ref.to_string(), 10)
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
fn split_balanced_args(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut last = 0;

    for (i, c) in s.chars().enumerate() {
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            ',' if depth_paren == 0 && depth_bracket == 0 => {
                parts.push(&s[last..i]);
                last = i + 1;
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
    for (i, c) in s.chars().enumerate() {
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
        // Clean up artifacts like "Blast radius: " (trailing colon+space after empty replacement)
        result = result.replace(":  ", ": ");
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

    if result_set.is_empty() {
        let is_interactive = matches!(&query.scope, Scope::Focus { node, .. } if node == "$clicked" || node == "$selected");
        if !is_interactive {
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

    // Resolve groups
    let mut groups_resolved = Vec::new();
    for group in &query.groups {
        let mut group_matched = Vec::new();
        for node_pattern in &group.nodes {
            let lower = node_pattern.to_lowercase();
            for (id, n) in &node_map {
                if result_set.contains(id)
                    && (n.qualified_name.to_lowercase().contains(&lower)
                        || n.name.to_lowercase().contains(&lower))
                {
                    group_matched.push(n.qualified_name.clone());
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

    // Resolve callouts
    let mut callouts_resolved = 0;
    let mut callouts_unresolved = Vec::new();
    for callout in &query.callouts {
        let lower = callout.node.to_lowercase();
        let found = node_map.values().any(|n| {
            n.qualified_name.to_lowercase().contains(&lower)
                || n.name.to_lowercase().contains(&lower)
        });
        if found {
            callouts_resolved += 1;
        } else {
            callouts_unresolved.push(callout.node.clone());
        }
    }

    // Resolve narrative
    let mut narrative_resolved = 0;
    for step in &query.narrative {
        let lower = step.node.to_lowercase();
        let found = node_map.values().any(|n| {
            n.qualified_name.to_lowercase().contains(&lower)
                || n.name.to_lowercase().contains(&lower)
        });
        if found {
            narrative_resolved += 1;
        }
    }

    // Apply edge filtering: restrict edges to connections between matched nodes
    let edge_type_filters: Vec<EdgeType> = query
        .edges
        .filter
        .iter()
        .filter_map(|s| parse_edge_type(s))
        .collect();
    const MAX_MATCHED_EDGES: usize = 1000;
    let matched_edges: Vec<MatchedEdge> = edges
        .iter()
        .filter(|e| e.deleted_at.is_none())
        .filter(|e| {
            result_set.contains(&e.source_id.to_string())
                && result_set.contains(&e.target_id.to_string())
        })
        .filter(|e| edge_type_filters.is_empty() || edge_type_filters.contains(&e.edge_type))
        .take(MAX_MATCHED_EDGES + 1)
        .map(|e| MatchedEdge {
            source_id: e.source_id.to_string(),
            target_id: e.target_id.to_string(),
            edge_type: edge_type_str(&e.edge_type).to_string(),
        })
        .collect();
    if matched_edges.len() > MAX_MATCHED_EDGES {
        warnings.push(format!(
            "Matched edges capped at {MAX_MATCHED_EDGES} (total: {} edges before truncation)",
            matched_edges.len()
        ));
    }
    let matched_edges: Vec<MatchedEdge> =
        matched_edges.into_iter().take(MAX_MATCHED_EDGES).collect();

    // Compute per-node metric values for heat emphasis (reuses pre-built adjacency)
    let node_metrics = if let Some(ref heat) = query.emphasis.heat {
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
                        node_map.get(id).map(|n| {
                            let churn = n.churn_count_30d as f64;
                            let complexity = n.complexity.unwrap_or(1) as f64;
                            let test_gap = 1.0 - n.test_coverage.unwrap_or(0.0);
                            churn * complexity * test_gap
                        })
                    }
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
        .into_iter()
        .take(10)
        .map(|(s, _)| s)
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
    let total_functions = active_nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Function)
        .count();
    let reachable_count = active_nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Function && reachable.contains(&n.id.to_string()))
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
}
