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
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start_id.to_string());
    queue.push_back((start_id.to_string(), 0u32));

    while let Some((current, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }

        let neighbors: Vec<&(String, EdgeType)> = match direction {
            "incoming" => incoming.get(&current).map(|v| v.iter().collect()).unwrap_or_default(),
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
            _ => outgoing.get(&current).map(|v| v.iter().collect()).unwrap_or_default(),
        };

        for (neighbor_id, et) in neighbors {
            if edge_types.is_empty() || edge_types.contains(et) {
                if visited.insert(neighbor_id.clone()) {
                    queue.push_back((neighbor_id.clone(), d + 1));
                }
            }
        }
    }

    visited
}

/// Find a node by name or qualified_name (case-insensitive partial match).
fn find_node_by_name<'a>(nodes: &'a [GraphNode], name: &str) -> Option<&'a GraphNode> {
    let lower = name.to_lowercase();
    // Exact match first.
    nodes
        .iter()
        .find(|n| n.qualified_name.to_lowercase() == lower || n.name.to_lowercase() == lower)
        .or_else(|| {
            // Partial match.
            nodes
                .iter()
                .find(|n| n.qualified_name.to_lowercase().contains(&lower) || n.name.to_lowercase().contains(&lower))
        })
}

/// Compute the set of nodes reachable from test functions via Calls edges.
fn compute_test_reachable(
    nodes: &[GraphNode],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashSet<String> {
    let test_node_ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.test_node && n.deleted_at.is_none())
        .map(|n| n.id.to_string())
        .collect();

    let mut reachable = HashSet::new();
    for test_id in &test_node_ids {
        let reached = bfs_traverse(
            test_id,
            &[EdgeType::Calls],
            "outgoing",
            100,
            outgoing,
            &HashMap::new(),
        );
        reachable.extend(reached);
    }
    reachable
}

// ── Core resolver ────────────────────────────────────────────────────────────

/// Resolve a view query scope against the graph, returning the set of matched node IDs.
pub fn resolve_scope(
    scope: &Scope,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
) -> HashSet<String> {
    let active_nodes: Vec<&GraphNode> = nodes.iter().filter(|n| n.deleted_at.is_none()).collect();
    let all_ids: HashSet<String> = active_nodes.iter().map(|n| n.id.to_string()).collect();
    let (outgoing, incoming) = build_adjacency(edges);

    match scope {
        Scope::All => all_ids,

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
                // Try direct ID match first, then name match.
                active_nodes
                    .iter()
                    .find(|n| n.id.to_string() == resolved_name)
                    .or_else(|| find_node_by_name(&active_nodes.iter().map(|n| (*n).clone()).collect::<Vec<_>>(), resolved_name).and_then(|found| active_nodes.iter().find(|n| n.id == found.id)))
            };

            match start_node {
                Some(sn) => {
                    let edge_types: Vec<EdgeType> = edge_strs
                        .iter()
                        .filter_map(|s| parse_edge_type(s))
                        .collect();
                    bfs_traverse(
                        &sn.id.to_string(),
                        &edge_types,
                        direction,
                        *depth,
                        &outgoing,
                        &incoming,
                    )
                }
                None => HashSet::new(),
            }
        }

        Scope::Filter {
            node_types,
            computed,
        } => {
            // If computed expression, try to evaluate it.
            if let Some(expr) = computed {
                resolve_computed_expression(expr, &active_nodes, edges, &outgoing, &incoming)
            } else {
                let types: Vec<NodeType> = node_types
                    .iter()
                    .filter_map(|s| parse_node_type(s))
                    .collect();
                if types.is_empty() {
                    all_ids
                } else {
                    active_nodes
                        .iter()
                        .filter(|n| types.contains(&n.node_type))
                        .map(|n| n.id.to_string())
                        .collect()
                }
            }
        }

        Scope::TestGaps => {
            let reachable = compute_test_reachable(
                &active_nodes.iter().map(|n| (*n).clone()).collect::<Vec<_>>(),
                &outgoing,
            );
            active_nodes
                .iter()
                .filter(|n| !reachable.contains(&n.id.to_string()))
                .filter(|n| n.node_type == NodeType::Function)
                .map(|n| n.id.to_string())
                .collect()
        }

        Scope::Diff { .. } => {
            // Diff requires temporal graph data; return all for now.
            all_ids
        }

        Scope::Concept {
            seed_nodes,
            expand_edges,
            expand_depth,
        } => {
            let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
            let edge_types: Vec<EdgeType> = expand_edges
                .iter()
                .filter_map(|s| parse_edge_type(s))
                .collect();
            let mut result = HashSet::new();

            for seed_name in seed_nodes {
                if let Some(seed) = find_node_by_name(&all_nodes_vec, seed_name) {
                    let reached = bfs_traverse(
                        &seed.id.to_string(),
                        &edge_types,
                        "both",
                        *expand_depth,
                        &outgoing,
                        &incoming,
                    );
                    result.extend(reached);
                }
            }
            result
        }
    }
}

/// Attempt to resolve simple computed expressions.
/// Supports: $test_unreachable, $test_reachable, $where(...), $intersect(...), $diff(...), $union(...)
fn resolve_computed_expression(
    expr: &str,
    active_nodes: &[&GraphNode],
    edges: &[GraphEdge],
    outgoing: &HashMap<String, Vec<(String, EdgeType)>>,
    _incoming: &HashMap<String, Vec<(String, EdgeType)>>,
) -> HashSet<String> {
    let trimmed = expr.trim();

    if trimmed == "$test_unreachable" {
        let all_nodes: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        let reachable = compute_test_reachable(&all_nodes, outgoing);
        return active_nodes
            .iter()
            .filter(|n| !reachable.contains(&n.id.to_string()))
            .filter(|n| n.node_type == NodeType::Function)
            .map(|n| n.id.to_string())
            .collect();
    }

    if trimmed == "$test_reachable" {
        let all_nodes: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        return compute_test_reachable(&all_nodes, outgoing);
    }

    // $where(property, op, value)
    if trimmed.starts_with("$where(") && trimmed.ends_with(')') {
        let inner = &trimmed[7..trimmed.len() - 1];
        let parts: Vec<&str> = inner.splitn(3, ',').map(|s| s.trim().trim_matches('\'').trim_matches('"')).collect();
        if parts.len() == 3 {
            let prop = parts[0];
            let op = parts[1];
            let val: f64 = parts[2].parse().unwrap_or(0.0);
            return active_nodes
                .iter()
                .filter(|n| {
                    let node_val = match prop {
                        "complexity" => n.complexity.map(|c| c as f64),
                        "churn" | "churn_count_30d" => Some(n.churn_count_30d as f64),
                        "test_coverage" => n.test_coverage,
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

    // $intersect(set_a, set_b) — simple two-argument form
    if trimmed.starts_with("$intersect(") && trimmed.ends_with(')') {
        let inner = &trimmed[11..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression(a_expr, active_nodes, edges, outgoing, _incoming);
            let set_b = resolve_computed_expression(b_expr, active_nodes, edges, outgoing, _incoming);
            return set_a.intersection(&set_b).cloned().collect();
        }
    }

    // $union(set_a, set_b)
    if trimmed.starts_with("$union(") && trimmed.ends_with(')') {
        let inner = &trimmed[7..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression(a_expr, active_nodes, edges, outgoing, _incoming);
            let set_b = resolve_computed_expression(b_expr, active_nodes, edges, outgoing, _incoming);
            return set_a.union(&set_b).cloned().collect();
        }
    }

    // $diff(set_a, set_b)
    if trimmed.starts_with("$diff(") && trimmed.ends_with(')') {
        let inner = &trimmed[6..trimmed.len() - 1];
        if let Some(comma_pos) = find_balanced_comma(inner) {
            let a_expr = inner[..comma_pos].trim();
            let b_expr = inner[comma_pos + 1..].trim();
            let set_a = resolve_computed_expression(a_expr, active_nodes, edges, outgoing, _incoming);
            let set_b = resolve_computed_expression(b_expr, active_nodes, edges, outgoing, _incoming);
            return set_a.difference(&set_b).cloned().collect();
        }
    }

    // $callers(node, depth?) — nodes with incoming Calls edges transitively
    if trimmed.starts_with("$callers(") && trimmed.ends_with(')') {
        let inner = &trimmed[9..trimmed.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').map(|s| s.trim()).collect();
        let node_name = parts[0].trim_matches('\'').trim_matches('"');
        let depth: u32 = parts.get(1).and_then(|d| d.parse().ok()).unwrap_or(10);
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Calls],
                "incoming",
                depth,
                outgoing,
                _incoming,
            );
        }
        return HashSet::new();
    }

    // $callees(node, depth?) — nodes with outgoing Calls edges transitively
    if trimmed.starts_with("$callees(") && trimmed.ends_with(')') {
        let inner = &trimmed[9..trimmed.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').map(|s| s.trim()).collect();
        let node_name = parts[0].trim_matches('\'').trim_matches('"');
        let depth: u32 = parts.get(1).and_then(|d| d.parse().ok()).unwrap_or(10);
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Calls],
                "outgoing",
                depth,
                outgoing,
                _incoming,
            );
        }
        return HashSet::new();
    }

    // $implementors(node) — types with Implements edges TO this node
    if trimmed.starts_with("$implementors(") && trimmed.ends_with(')') {
        let node_name = trimmed[14..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            let found_id = found.id.to_string();
            return _incoming.get(&found_id)
                .map(|neighbors| {
                    neighbors.iter()
                        .filter(|(_, et)| *et == EdgeType::Implements)
                        .map(|(id, _)| id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
        }
        return HashSet::new();
    }

    // $fields(node) — nodes with FieldOf edges TO this node
    if trimmed.starts_with("$fields(") && trimmed.ends_with(')') {
        let node_name = trimmed[8..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            let found_id = found.id.to_string();
            // FieldOf: source is the field, target is the parent type
            return _incoming.get(&found_id)
                .map(|neighbors| {
                    neighbors.iter()
                        .filter(|(_, et)| *et == EdgeType::FieldOf)
                        .map(|(id, _)| id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
        }
        return HashSet::new();
    }

    // $descendants(node) — all children via Contains edges recursively
    if trimmed.starts_with("$descendants(") && trimmed.ends_with(')') {
        let node_name = trimmed[13..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Contains],
                "outgoing",
                100,
                outgoing,
                _incoming,
            );
        }
        return HashSet::new();
    }

    // $ancestors(node) — parent chain via Contains edges to root
    if trimmed.starts_with("$ancestors(") && trimmed.ends_with(')') {
        let node_name = trimmed[11..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            return bfs_traverse(
                &found.id.to_string(),
                &[EdgeType::Contains],
                "incoming",
                100,
                outgoing,
                _incoming,
            );
        }
        return HashSet::new();
    }

    // $governed_by(spec_path) — nodes linked to a spec via GovernedBy
    if trimmed.starts_with("$governed_by(") && trimmed.ends_with(')') {
        let spec_path = trimmed[13..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let lower = spec_path.to_lowercase();
        return active_nodes
            .iter()
            .filter(|n| {
                n.spec_path.as_ref().map_or(false, |sp| sp.to_lowercase().contains(&lower))
            })
            .map(|n| n.id.to_string())
            .collect();
    }

    // $test_fragility(node) — count of distinct test paths reaching this node (returns as single-item set for use in intersect)
    if trimmed.starts_with("$test_fragility(") && trimmed.ends_with(')') {
        let node_name = trimmed[16..trimmed.len() - 1].trim().trim_matches('\'').trim_matches('"');
        let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
        if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
            let found_id = found.id.to_string();
            // Count how many test nodes can reach this node
            let test_ids: Vec<String> = active_nodes
                .iter()
                .filter(|n| n.test_node)
                .map(|n| n.id.to_string())
                .collect();
            let mut count = 0;
            for tid in &test_ids {
                let reached = bfs_traverse(tid, &[EdgeType::Calls], "outgoing", 100, outgoing, _incoming);
                if reached.contains(&found_id) {
                    count += 1;
                }
            }
            // Return the node itself if it has any test coverage
            if count > 0 {
                let mut result = HashSet::new();
                result.insert(found_id);
                return result;
            }
        }
        return HashSet::new();
    }

    // $reachable(node, edge_types, direction, depth) — general BFS
    if trimmed.starts_with("$reachable(") && trimmed.ends_with(')') {
        let inner = &trimmed[11..trimmed.len() - 1];
        let parts: Vec<&str> = inner.splitn(4, ',').map(|s| s.trim()).collect();
        if parts.len() >= 2 {
            let node_name = parts[0].trim_matches('\'').trim_matches('"');
            let edge_types_str = parts.get(1).unwrap_or(&"");
            let direction = parts.get(2).map(|s| s.trim_matches('\'').trim_matches('"')).unwrap_or("outgoing");
            let depth: u32 = parts.get(3).and_then(|d| d.parse().ok()).unwrap_or(10);

            let edge_types: Vec<EdgeType> = edge_types_str
                .trim_matches(|c: char| c == '[' || c == ']')
                .split(',')
                .filter_map(|s| parse_edge_type(s.trim().trim_matches('\'').trim_matches('"')))
                .collect();

            let all_nodes_vec: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
            if let Some(found) = find_node_by_name(&all_nodes_vec, node_name) {
                return bfs_traverse(&found.id.to_string(), &edge_types, direction, depth, outgoing, _incoming);
            }
        }
        return HashSet::new();
    }

    // Fallback: empty set
    HashSet::new()
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

// ── Dry-run ──────────────────────────────────────────────────────────────────

/// Run a complete dry-run of a view query, producing match counts and warnings.
pub fn dry_run(
    query: &ViewQuery,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    selected_node_id: Option<&str>,
) -> DryRunResult {
    let mut warnings = Vec::new();

    // Resolve scope
    let result_set = resolve_scope(&query.scope, nodes, edges, selected_node_id);

    if result_set.is_empty() {
        // Only warn if the query wasn't using interactive binding
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

    let matched_node_names: Vec<String> = result_set
        .iter()
        .filter_map(|id| node_map.get(id).map(|n| n.qualified_name.clone()))
        .take(50) // Cap for readability
        .collect();

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

    DryRunResult {
        matched_nodes: result_set.len(),
        matched_node_names,
        groups_resolved,
        callouts_resolved,
        callouts_unresolved,
        narrative_resolved,
        warnings,
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
        *node_counts.entry(node_type_str(&n.node_type).to_string()).or_default() += 1;
    }

    let mut edge_counts: HashMap<String, usize> = HashMap::new();
    for e in &active_edges {
        *edge_counts.entry(edge_type_str(&e.edge_type).to_string()).or_default() += 1;
    }

    // Top types by field count
    let mut field_counts: HashMap<String, usize> = HashMap::new();
    for e in &active_edges {
        if e.edge_type == EdgeType::FieldOf {
            *field_counts.entry(e.source_id.to_string()).or_default() += 1;
        }
    }
    let node_map: HashMap<String, &GraphNode> = active_nodes.iter().map(|n| (n.id.to_string(), *n)).collect();
    let mut type_field_pairs: Vec<(String, usize)> = field_counts
        .iter()
        .filter_map(|(id, count)| {
            node_map.get(id).map(|n| (format!("{} ({} fields)", n.name, count), *count))
        })
        .collect();
    type_field_pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let top_types: Vec<String> = type_field_pairs.into_iter().take(10).map(|(s, _)| s).collect();

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
            node_map.get(id).map(|n| (format!("{} ({})", n.name, count), *count))
        })
        .collect();
    func_call_pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let top_functions: Vec<String> = func_call_pairs.into_iter().take(10).map(|(s, _)| s).collect();

    // Modules
    let modules: Vec<String> = active_nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Module)
        .map(|n| n.qualified_name.clone())
        .collect();

    // Test coverage
    let (outgoing, _) = build_adjacency(edges);
    let test_functions = active_nodes.iter().filter(|n| n.test_node).count();
    let all_nodes_owned: Vec<GraphNode> = active_nodes.iter().map(|n| (*n).clone()).collect();
    let reachable = compute_test_reachable(&all_nodes_owned, &outgoing);
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
        // A -> B -> C (depth 2), D is at depth 3
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
        assert_eq!(result.len(), 3); // C + A + B
    }

    #[test]
    fn test_scope_test_gaps() {
        let nodes = vec![
            make_node("n1", "tested_fn", NodeType::Function),
            make_node("n2", "untested_fn", NodeType::Function),
            make_test_node("t1", "test_something"),
        ];
        let edges = vec![
            make_edge("e1", "t1", "n1", EdgeType::Calls),
        ];
        let result = resolve_scope(&Scope::TestGaps, &nodes, &edges, None);
        assert!(result.contains("n2"));
        assert!(!result.contains("n1"));
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
            },
            &nodes,
            &edges,
            None,
        );
        assert!(result.contains("n1")); // seed
        assert!(result.contains("n2")); // outgoing call
        assert!(result.contains("n3")); // incoming call (both direction)
        assert!(!result.contains("n4")); // unrelated
    }

    #[test]
    fn test_computed_test_unreachable() {
        let nodes = vec![
            make_node("n1", "reachable", NodeType::Function),
            make_node("n2", "unreachable", NodeType::Function),
            make_test_node("t1", "test_fn"),
        ];
        let edges = vec![
            make_edge("e1", "t1", "n1", EdgeType::Calls),
        ];
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&edges);
        let result = resolve_computed_expression("$test_unreachable", &active, &edges, &outgoing, &incoming);
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
        let result = resolve_computed_expression("$where(complexity, '>', 20)", &active, &[], &outgoing, &incoming);
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
        );
        assert!(result.contains("n1")); // complex AND untested
        assert!(!result.contains("n2")); // simple
        assert!(!result.contains("n3")); // complex but tested
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
        let result = resolve_computed_expression("$callers(C)", &active, &edges, &outgoing, &incoming);
        assert!(result.contains("n1"));
        assert!(result.contains("n2"));
        assert!(result.contains("n3")); // start node
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
        let result = resolve_computed_expression("$callees(A)", &active, &edges, &outgoing, &incoming);
        assert!(result.contains("n1")); // start node
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
        let result = resolve_computed_expression("$implementors(MyTrait)", &active, &edges, &outgoing, &incoming);
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
        let result = resolve_computed_expression("$descendants(root_mod)", &active, &edges, &outgoing, &incoming);
        assert!(result.contains("n1")); // start
        assert!(result.contains("n2"));
        assert!(result.contains("n3"));
    }

    #[test]
    fn test_computed_governed_by() {
        let mut nodes = vec![
            make_node("n1", "Governed", NodeType::Type),
            make_node("n2", "Ungoverned", NodeType::Type),
        ];
        nodes[0].spec_path = Some("specs/search.md".to_string());
        let active: Vec<&GraphNode> = nodes.iter().collect();
        let (outgoing, incoming) = build_adjacency(&[]);
        let result = resolve_computed_expression("$governed_by(search.md)", &active, &[], &outgoing, &incoming);
        assert!(result.contains("n1"));
        assert!(!result.contains("n2"));
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

        // Union of types and functions = all
        let union = resolve_computed_expression(
            "$union($where(complexity, '>=', 0), $where(complexity, '<', 0))",
            &active, &[], &outgoing, &incoming,
        );
        // $where(complexity >= 0) should match all (complexity=5 default)
        assert!(union.contains("n1"));
        assert!(union.contains("n2"));
        assert!(union.contains("n3"));

        // Diff: all - complex = simple
        let mut nodes_varied = nodes.clone();
        nodes_varied[0].complexity = Some(30);
        nodes_varied[1].complexity = Some(3);
        nodes_varied[2].complexity = Some(25);
        let active2: Vec<&GraphNode> = nodes_varied.iter().collect();
        let diff = resolve_computed_expression(
            "$diff($where(complexity, '>', 0), $where(complexity, '>', 20))",
            &active2, &[], &outgoing, &incoming,
        );
        assert!(diff.contains("n2")); // complexity 3, not > 20
        assert!(!diff.contains("n1")); // complexity 30, > 20
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
            &active, &edges, &outgoing, &incoming,
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
}
