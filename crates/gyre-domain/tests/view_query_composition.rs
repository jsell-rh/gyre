//! Integration tests for the canonical composition examples in
//! `specs/system/view-query-grammar.md` §11–15.
//!
//! Each test constructs the EXACT query JSON from the spec, deserializes it
//! into a `ViewQuery`, and resolves it against a synthetic knowledge graph via
//! `dry_run`, asserting the result set, emphasis data (tiered depths / heat
//! metrics), matched edges, and annotation template resolution.
//!
//! These tests exercise the full grammar pipeline end-to-end: JSON shape →
//! ViewQuery deserialization → validation → scope resolution → computed
//! expressions → emphasis → annotation templating.

use gyre_common::graph::{EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence, Visibility};
use gyre_common::view_query::ViewQuery;
use gyre_common::Id;
use gyre_domain::view_query_resolver::dry_run;

// ── Synthetic graph helpers ─────────────────────────────────────────────────

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

fn with_complexity(mut n: GraphNode, c: u32) -> GraphNode {
    n.complexity = Some(c);
    n
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

fn parse_query(json: &str) -> ViewQuery {
    let query: ViewQuery = serde_json::from_str(json).expect("spec JSON must deserialize");
    let errors = query.validate();
    assert!(
        errors.is_empty(),
        "spec query must validate cleanly, got errors: {errors:?}"
    );
    query
}

// ── §12 Blast radius (interactive) ──────────────────────────────────────────

/// Graph shape (Calls edges point caller → callee):
///
///   handler ──calls──▶ TaskPort ──calls──▶ store
///   impl_a  ──implements──▶ TaskPort (trait)
///   unrelated ──calls──▶ other (NOT in blast radius)
///   handler ──calls──▶ side (depth 2 via incoming from TaskPort? no -
///   side is called BY handler, so it is NOT an incoming caller of TaskPort)
const BLAST_RADIUS_QUERY: &str = r##"{
  "scope": { "type": "focus", "node": "$clicked", "edges": ["calls", "implements"], "direction": "incoming", "depth": 10 },
  "emphasis": { "tiered_colors": ["#ef4444", "#f97316", "#eab308", "#94a3b8"], "dim_unmatched": 0.12 },
  "edges": { "filter": ["calls", "implements"] },
  "zoom": "fit",
  "annotation": { "title": "Blast radius: $name", "description": "{{count}} transitive callers/implementors" }
}"##;

fn blast_radius_graph() -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let task_port = make_node("n-taskport", "TaskPort", NodeType::Trait);
    let nodes = vec![
        make_node("n-handler", "handler", NodeType::Function),
        task_port,
        make_node("n-store", "store", NodeType::Function),
        make_node("n-impl-a", "ImplA", NodeType::Type),
        make_node("n-impl-b", "ImplB", NodeType::Type),
        make_node("n-unrelated", "unrelated", NodeType::Function),
        make_node("n-other", "other", NodeType::Function),
    ];
    // handler calls TaskPort's method surface; a deeper caller calls handler.
    let edges = vec![
        make_edge("e1", "n-handler", "n-taskport", EdgeType::Calls),
        make_edge("e2", "n-impl-a", "n-taskport", EdgeType::Implements),
        make_edge("e3", "n-impl-b", "n-taskport", EdgeType::Implements),
        make_edge("e4", "n-unrelated", "n-other", EdgeType::Calls),
    ];
    (nodes, edges)
}

#[test]
fn blast_radius_interactive_resolves_from_clicked_node() {
    let (nodes, edges) = blast_radius_graph();
    let query = parse_query(BLAST_RADIUS_QUERY);

    // The user clicked TaskPort: $clicked resolves to it via selected_node_id.
    let result = dry_run(&query, &nodes, &edges, Some("n-taskport"));

    // Result set: TaskPort itself + incoming callers (handler) + implementors.
    assert_eq!(
        result.matched_nodes, 4,
        "matched: {:?}",
        result.matched_node_names
    );
    assert!(result.matched_node_names.contains(&"TaskPort".to_string()));
    assert!(result.matched_node_names.contains(&"handler".to_string()));
    assert!(result.matched_node_names.contains(&"ImplA".to_string()));
    assert!(result.matched_node_names.contains(&"ImplB".to_string()));
    assert!(
        !result.matched_node_names.contains(&"unrelated".to_string()),
        "nodes with no path to the clicked node must be excluded"
    );
    assert!(
        !result.matched_node_names.contains(&"other".to_string()),
        "nodes only reachable from unrelated subgraphs must be excluded"
    );

    // Emphasis: tiered_colors requires per-node BFS depths.
    assert_eq!(
        result.node_depths.get("n-taskport"),
        Some(&0),
        "clicked node is depth 0"
    );
    assert_eq!(result.node_depths.get("n-handler"), Some(&1));
    assert_eq!(result.node_depths.get("n-impl-a"), Some(&1));
    assert_eq!(result.node_depths.get("n-impl-b"), Some(&1));

    // Edges: restricted to connections BETWEEN matched nodes, filtered to
    // calls+implements. The unrelated→other Calls edge must NOT appear.
    assert_eq!(result.matched_edges.len(), 3);
    for e in &result.matched_edges {
        assert!(
            e.source_id != "n-unrelated" && e.target_id != "n-other",
            "edges outside the result set must not leak: {:?}",
            e
        );
    }

    // Annotation templates resolve: $name → clicked node, {{count}} → 4.
    let ann = result.resolved_annotation.expect("annotation must resolve");
    assert_eq!(ann.title.as_deref(), Some("Blast radius: TaskPort"));
    assert_eq!(
        ann.description.as_deref(),
        Some("4 transitive callers/implementors")
    );
}

#[test]
fn blast_radius_interactive_reruns_from_each_click() {
    // §7/§12: "$clicked" makes the query a MODE - each click re-runs the
    // traversal from the newly clicked node. Resolving the same query with a
    // different selected_node_id must produce that node's blast radius.
    let (nodes, edges) = blast_radius_graph();
    let query = parse_query(BLAST_RADIUS_QUERY);

    let from_taskport = dry_run(&query, &nodes, &edges, Some("n-taskport"));
    let from_impl_a = dry_run(&query, &nodes, &edges, Some("n-impl-a"));

    assert_eq!(from_taskport.matched_nodes, 4);
    // ImplA has no incoming calls/implements edges: blast radius is just itself.
    assert_eq!(from_impl_a.matched_nodes, 1);
    assert!(from_impl_a
        .matched_node_names
        .contains(&"ImplA".to_string()));

    // Annotation updates with the new node's name.
    let ann = from_impl_a
        .resolved_annotation
        .expect("annotation must resolve");
    assert_eq!(ann.title.as_deref(), Some("Blast radius: ImplA"));
}

#[test]
fn blast_radius_interactive_without_selection_warns() {
    let (nodes, edges) = blast_radius_graph();
    let query = parse_query(BLAST_RADIUS_QUERY);
    let result = dry_run(&query, &nodes, &edges, None);
    assert_eq!(result.matched_nodes, 0);
    assert!(
        result.warnings.iter().any(|w| w.contains("click a node")),
        "interactive query with no selection must tell the user to click: {:?}",
        result.warnings
    );
}

// ── §13 Test coverage gaps ──────────────────────────────────────────────────

const TEST_GAPS_QUERY: &str = r##"{
  "scope": { "type": "test_gaps" },
  "emphasis": { "highlight": { "matched": { "color": "#ef4444", "label": "Untested" }}, "dim_unmatched": 0.3 },
  "annotation": { "title": "Test coverage gaps", "description": "{{count}} functions not reachable from any test" }
}"##;

#[test]
fn test_coverage_gaps_identifies_unreachable_functions() {
    // test_fn calls covered_fn. orphan_fn and risky_fn are never called by tests.
    let nodes = vec![
        make_test_node("n-test", "test_fn"),
        make_node("n-covered", "covered_fn", NodeType::Function),
        make_node("n-orphan", "orphan_fn", NodeType::Function),
        make_node("n-risky", "risky_fn", NodeType::Function),
        // Non-testable node types are excluded from gaps even when untested.
        make_node("n-const", "MAX_RETRIES", NodeType::Constant),
    ];
    let edges = vec![
        make_edge("e1", "n-test", "n-covered", EdgeType::Calls),
        // Implements does NOT confer test reachability (Calls-only per §3).
        make_edge("e2", "n-test", "n-risky", EdgeType::Implements),
    ];
    let query = parse_query(TEST_GAPS_QUERY);
    let result = dry_run(&query, &nodes, &edges, None);

    assert_eq!(
        result.matched_nodes, 2,
        "matched: {:?}",
        result.matched_node_names
    );
    assert!(result.matched_node_names.contains(&"orphan_fn".to_string()));
    assert!(
        result.matched_node_names.contains(&"risky_fn".to_string()),
        "Implements edges must not count as test reachability"
    );
    assert!(
        !result
            .matched_node_names
            .contains(&"covered_fn".to_string()),
        "functions called from tests are covered, not gaps"
    );
    assert!(
        !result
            .matched_node_names
            .contains(&"MAX_RETRIES".to_string()),
        "non-testable node types are not coverage gaps"
    );

    let ann = result.resolved_annotation.expect("annotation must resolve");
    assert_eq!(ann.title.as_deref(), Some("Test coverage gaps"));
    assert_eq!(
        ann.description.as_deref(),
        Some("2 functions not reachable from any test")
    );
}

// ── §14 High-risk untested code ─────────────────────────────────────────────

const HIGH_RISK_QUERY: &str = r##"{
  "scope": { "type": "filter", "computed": "$intersect($where(complexity, '>', 20), $test_unreachable)" },
  "emphasis": { "heat": { "metric": "complexity", "palette": "blue-red" }, "dim_unmatched": 0.15 },
  "annotation": { "title": "High-risk untested code", "description": "{{count}} complex functions with no test coverage" }
}"##;

#[test]
fn high_risk_untested_intersects_complexity_and_test_gaps() {
    // Test reachability chain: test_fn → mid_fn → covered_complex_fn.
    let nodes = vec![
        make_test_node("n-test", "test_fn"),
        with_complexity(make_node("n-hot", "hot_fn", NodeType::Function), 35),
        // Complex but covered by a test → excluded by $test_unreachable.
        with_complexity(
            make_node(
                "n-covered-complex",
                "covered_complex_fn",
                NodeType::Function,
            ),
            30,
        ),
        // Untested but simple → excluded by $where(complexity, '>', 20).
        with_complexity(make_node("n-simple", "simple_fn", NodeType::Function), 3),
        // Untested and exactly at the boundary (20) → '>' is strict, excluded.
        with_complexity(
            make_node("n-boundary", "boundary_fn", NodeType::Function),
            20,
        ),
        with_complexity(make_node("n-mid", "mid_fn", NodeType::Function), 8),
    ];
    let mut edges = vec![make_edge("e1", "n-test", "n-mid", EdgeType::Calls)];
    edges.push(make_edge(
        "e2",
        "n-mid",
        "n-covered-complex",
        EdgeType::Calls,
    ));

    // Sanity on the synthetic graph itself: the covered chain is reachable.
    assert_eq!(nodes.len(), 6);
    assert_eq!(edges.len(), 2);

    let query = parse_query(HIGH_RISK_QUERY);
    let result = dry_run(&query, &nodes, &edges, None);

    // Only hot_fn is both complex (>20) and unreachable from tests.
    assert_eq!(
        result.matched_nodes, 1,
        "matched: {:?}",
        result.matched_node_names
    );
    assert!(result.matched_node_names.contains(&"hot_fn".to_string()));
    assert!(!result
        .matched_node_names
        .contains(&"covered_complex_fn".to_string()));
    assert!(!result.matched_node_names.contains(&"simple_fn".to_string()));
    assert!(!result
        .matched_node_names
        .contains(&"boundary_fn".to_string()));

    // Emphasis: heat map by complexity - every matched node has a metric value.
    assert_eq!(result.node_metrics.get("n-hot"), Some(&35.0));

    let ann = result.resolved_annotation.expect("annotation must resolve");
    assert_eq!(ann.title.as_deref(), Some("High-risk untested code"));
    assert_eq!(
        ann.description.as_deref(),
        Some("1 complex functions with no test coverage")
    );
}

// ── §15 Authentication concept ──────────────────────────────────────────────

const AUTH_CONCEPT_QUERY: &str = r##"{
  "scope": { "type": "concept", "seed_nodes": ["AuthService", "TokenStore"], "expand_edges": ["calls"], "expand_depth": 2 },
  "emphasis": { "highlight": { "matched": { "color": "#60a5fa" }}, "dim_unmatched": 0.15 },
  "zoom": "fit",
  "annotation": { "title": "Authentication system", "description": "{{count}} nodes" }
}"##;

#[test]
fn auth_concept_expands_from_seed_nodes() {
    // Concept graph (Calls = caller → callee):
    //   login_handler ──▶ AuthService ──▶ TokenStore ──▶ crypto_util   (chain from seeds)
    //   audit_log ◀── AuthService (incoming to seed)
    //   distant_fn ──▶ crypto_util at depth 3 from AuthService (beyond expand_depth 2)
    //   unrelated_fn ──▶ unrelated_other (disconnected)
    let nodes = vec![
        make_node("n-auth", "AuthService", NodeType::Type),
        make_node("n-token", "TokenStore", NodeType::Type),
        make_node("n-login", "login_handler", NodeType::Function),
        make_node("n-audit", "audit_log", NodeType::Function),
        make_node("n-crypto", "crypto_util", NodeType::Function),
        make_node("n-distant", "distant_fn", NodeType::Function),
        make_node("n-unrelated", "unrelated_fn", NodeType::Function),
        make_node("n-unrelated-other", "unrelated_other", NodeType::Function),
    ];
    let edges = vec![
        make_edge("e1", "n-login", "n-auth", EdgeType::Calls),
        make_edge("e2", "n-auth", "n-token", EdgeType::Calls),
        make_edge("e3", "n-auth", "n-audit", EdgeType::Calls),
        make_edge("e4", "n-token", "n-crypto", EdgeType::Calls),
        make_edge("e5", "n-distant", "n-crypto", EdgeType::Calls),
        make_edge("e6", "n-unrelated", "n-unrelated-other", EdgeType::Calls),
    ];
    let query = parse_query(AUTH_CONCEPT_QUERY);
    let result = dry_run(&query, &nodes, &edges, None);

    // From AuthService (depth 2): login_handler(1), TokenStore(1), audit_log(1),
    // crypto_util(2). From TokenStore (depth 2): AuthService(1), crypto_util(1),
    // distant_fn(2). Union: 7 nodes minus the disconnected unrelated pair (2).
    let expected = [
        "AuthService",
        "TokenStore",
        "login_handler",
        "audit_log",
        "crypto_util",
        "distant_fn",
    ];
    assert_eq!(
        result.matched_nodes,
        expected.len(),
        "matched: {:?}",
        result.matched_node_names
    );
    for name in expected {
        assert!(
            result.matched_node_names.contains(&name.to_string()),
            "expected {} in concept expansion",
            name
        );
    }
    assert!(
        !result
            .matched_node_names
            .contains(&"unrelated_fn".to_string()),
        "disconnected subgraphs must not appear in the concept view"
    );
    assert!(
        !result
            .matched_node_names
            .contains(&"unrelated_other".to_string()),
        "disconnected subgraphs must not appear in the concept view"
    );

    // Depths are per-seed minimums: crypto_util is depth 1 from TokenStore.
    assert_eq!(result.node_depths.get("n-crypto"), Some(&1));
    assert_eq!(result.node_depths.get("n-auth"), Some(&0));
    assert_eq!(result.node_depths.get("n-token"), Some(&0));

    let ann = result.resolved_annotation.expect("annotation must resolve");
    assert_eq!(ann.title.as_deref(), Some("Authentication system"));
    assert_eq!(ann.description.as_deref(), Some("6 nodes"));
}

#[test]
fn auth_concept_partial_seed_resolution_warns() {
    let nodes = vec![
        make_node("n-auth", "AuthService", NodeType::Type),
        make_node("n-token", "TokenStore", NodeType::Type),
    ];
    let edges = vec![make_edge("e1", "n-auth", "n-token", EdgeType::Calls)];
    // Only one of the two seeds exists.
    let partial_json = r##"{
      "scope": { "type": "concept", "seed_nodes": ["AuthService", "MissingService"], "expand_edges": ["calls"], "expand_depth": 2 },
      "emphasis": { "highlight": { "matched": { "color": "#60a5fa" }}, "dim_unmatched": 0.15 },
      "zoom": "fit",
      "annotation": { "title": "Authentication system", "description": "{{count}} nodes" }
    }"##;
    let query = parse_query(partial_json);
    let result = dry_run(&query, &nodes, &edges, None);
    assert_eq!(result.matched_nodes, 2);
    assert!(
        result.warnings.iter().any(|w| w.contains("MissingService")),
        "missing seed node must produce a warning: {:?}",
        result.warnings
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("1 of 2 seed nodes")),
        "partial resolution must be reported: {:?}",
        result.warnings
    );
}

// ── System default views match §11–15 shapes ────────────────────────────────

#[test]
fn system_default_view_shapes_resolve() {
    // The exact query JSON shipped as system default views for the
    // composition examples (saved_views.rs) must resolve against a synthetic
    // graph: "Blast Radius (click)" (§12), "Test Coverage Gaps" (§13),
    // "High-Risk Untested Code" (§14).
    let (nodes, edges) = blast_radius_graph();
    let mut nodes = nodes;
    nodes.push(make_test_node("n-test", "test_fn"));
    nodes.push(with_complexity(
        make_node("n-risky", "risky_fn", NodeType::Function),
        30,
    ));
    // test_fn → handler makes handler test-reachable.
    let mut edges = edges;
    edges.push(make_edge("e5", "n-test", "n-handler", EdgeType::Calls));

    let defaults: Vec<(&str, &str)> = vec![
        (
            "Blast Radius (click)",
            r##"{"scope":{"type":"focus","node":"$clicked","edges":["calls","implements","field_of","depends_on"],"direction":"incoming","depth":10},"emphasis":{"tiered_colors":["#ef4444","#f97316","#eab308","#94a3b8"],"dim_unmatched":0.12},"edges":{"filter":["calls","implements","field_of","depends_on"]},"zoom":"fit","annotation":{"title":"Blast radius: $name","description":"{{count}} transitive callers/implementors"}}"##,
        ),
        (
            "Test Coverage Gaps",
            r##"{"scope":{"type":"test_gaps"},"emphasis":{"highlight":{"matched":{"color":"#ef4444","label":"Untested"}},"dim_unmatched":0.3},"annotation":{"title":"Test coverage gaps","description":"{{count}} functions not reachable from any test"}}"##,
        ),
        (
            "High-Risk Untested Code",
            r##"{"scope":{"type":"filter","computed":"$intersect($where(complexity, '>', 20), $test_unreachable)"},"emphasis":{"heat":{"metric":"complexity","palette":"blue-red"},"dim_unmatched":0.15},"annotation":{"title":"High-risk untested code","description":"{{count}} complex functions with no test coverage"}}"##,
        ),
    ];

    for (name, json) in defaults {
        let query = parse_query(json);
        let result = dry_run(&query, &nodes, &edges, Some("n-taskport"));
        assert!(
            result.matched_nodes > 0,
            "default view '{}' must match nodes",
            name
        );
        let ann = result
            .resolved_annotation
            .unwrap_or_else(|| panic!("default view '{}' must resolve its annotation", name));
        assert!(
            ann.description
                .as_deref()
                .unwrap_or_default()
                .contains(&result.matched_nodes.to_string()),
            "default view '{}' description must have {{{{count}}}} filled: {:?}",
            name,
            ann.description
        );
    }
}
