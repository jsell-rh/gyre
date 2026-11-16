//! task-164: the dependency graph endpoints enrich edges with version drift
//! and breaking-change detail (dependency-graph.md §CLI `deps show` table and
//! §UI dependency detail panel).
//!
//! Proves, against a real SQLite-backed state, that:
//!   - `GET /api/v1/dependencies/graph` and
//!     `GET /api/v1/workspaces/:id/dependency-graph` return edges carrying
//!     source/target artifacts, pinned/current versions, drift, and the
//!     breaking changes recorded against the edge with acknowledgment status;
//!   - breaking changes are matched to edges via the repo that introduced the
//!     change (the edge's TARGET — `BreakingChange.source_repo_id`), so a
//!     wrong-side lookup yields no breaking changes and this test fails;
//!   - node names are resolved from the repo store, not echoed as IDs.
//!
//! Own test binary (own process): `build_state` reads `GYRE_DATABASE_URL`
//! from the process environment.

use axum::extract::{Path, State};
use axum::Json;
use gyre_common::Id;
use gyre_domain::{
    BreakingChange, DependencyEdge, DependencyStatus, DependencyType, DetectionMethod, Repository,
    Workspace,
};
use gyre_server::api::dependencies::{get_graph, get_workspace_dependency_graph};
use gyre_server::build_state;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn graph_edges_carry_version_and_breaking_change_detail() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("task164.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let state = build_state("task164-token", "http://127.0.0.1:1", None);

    // Workspace with two repos: app depends on lib.
    let ws = Workspace::new(
        Id::new("ws-1"),
        Id::new("tenant-1"),
        "Graph WS",
        "graph-ws",
        1,
    );
    state.workspaces.create(&ws).await.unwrap();
    let repo_app = Repository::new(
        Id::new("repo-app"),
        Id::new("ws-1"),
        "app",
        "tenant-1/graph-ws/app",
        1,
    );
    let repo_lib = Repository::new(
        Id::new("repo-lib"),
        Id::new("ws-1"),
        "lib",
        "tenant-1/graph-ws/lib",
        1,
    );
    state.repos.create(&repo_app).await.unwrap();
    state.repos.create(&repo_lib).await.unwrap();

    // Dependency edge with version drift, in Breaking status.
    let mut edge = DependencyEdge::new(
        Id::new("edge-1"),
        Id::new("repo-app"),
        Id::new("repo-lib"),
        DependencyType::Code,
        "Cargo.toml",
        "lib",
        DetectionMethod::CargoToml,
        1_700_000_000,
    );
    edge.version_pinned = Some("1.2.0".to_string());
    edge.target_version_current = Some("1.5.0".to_string());
    edge.version_drift = Some(3);
    edge.status = DependencyStatus::Breaking;
    state.dependencies.save(&edge).await.unwrap();

    // Breaking changes introduced by repo-lib (the edge target): one
    // unacknowledged, one acknowledged.
    let bc_unack = BreakingChange::new(
        Id::new("bc-1"),
        edge.id.clone(),
        Id::new("repo-lib"),
        "aaaa0000bbbb0000",
        "switched to async API",
        1_700_000_100,
    );
    let mut bc_ack = BreakingChange::new(
        Id::new("bc-2"),
        edge.id.clone(),
        Id::new("repo-lib"),
        "cccc0000dddd0000",
        "removed legacy endpoint",
        1_700_000_150,
    );
    bc_ack.acknowledged = true;
    bc_ack.acknowledged_by = Some("admin".to_string());
    bc_ack.acknowledged_at = Some(1_700_000_200);
    state.breaking_changes.create(&bc_unack).await.unwrap();
    state.breaking_changes.create(&bc_ack).await.unwrap();

    // ── Tenant-wide graph ────────────────────────────────────────────────
    let Json(graph) = get_graph(State(state.clone())).await.unwrap();
    assert_eq!(graph.edges.len(), 1, "exactly one edge in the graph");
    let e = &graph.edges[0];
    assert_eq!(e.id, "edge-1");
    assert_eq!(e.source, "repo-app");
    assert_eq!(e.target, "repo-lib");
    assert_eq!(e.source_artifact, "Cargo.toml");
    assert_eq!(e.target_artifact, "lib");
    assert_eq!(e.version_pinned.as_deref(), Some("1.2.0"));
    assert_eq!(e.target_version_current.as_deref(), Some("1.5.0"));
    assert_eq!(e.version_drift, Some(3));
    assert_eq!(e.status, DependencyStatus::Breaking);

    // Breaking changes joined via the introducing (target) repo, with
    // acknowledgment status preserved. A wrong-side (source-repo) lookup
    // yields an empty list and fails here.
    assert_eq!(
        e.breaking_changes.len(),
        2,
        "both breaking changes must be attached to the edge"
    );
    let unack = e
        .breaking_changes
        .iter()
        .find(|b| b.id == "bc-1")
        .expect("unacknowledged breaking change must be present");
    assert!(!unack.acknowledged);
    assert_eq!(unack.commit_sha, "aaaa0000bbbb0000");
    assert_eq!(unack.description, "switched to async API");
    let ack = e
        .breaking_changes
        .iter()
        .find(|b| b.id == "bc-2")
        .expect("acknowledged breaking change must be present");
    assert!(ack.acknowledged);
    assert_eq!(ack.acknowledged_by.as_deref(), Some("admin"));
    assert_eq!(ack.acknowledged_at, Some(1_700_000_200));

    // Node names resolved from the repo store.
    let names: Vec<(&str, &str)> = graph
        .nodes
        .iter()
        .map(|n| (n.repo_id.as_str(), n.name.as_str()))
        .collect();
    assert!(names.contains(&("repo-app", "app")));
    assert!(names.contains(&("repo-lib", "lib")));

    // ── Workspace-scoped graph ───────────────────────────────────────────
    let Json(ws_graph) =
        get_workspace_dependency_graph(State(state.clone()), Path("ws-1".to_string()))
            .await
            .unwrap();
    assert_eq!(ws_graph.edges.len(), 1);
    let we = &ws_graph.edges[0];
    assert_eq!(we.id, "edge-1");
    assert_eq!(we.version_drift, Some(3));
    assert_eq!(we.breaking_changes.len(), 2);

    // A workspace with no dependency edges returns an empty graph.
    let ws2 = Workspace::new(
        Id::new("ws-2"),
        Id::new("tenant-1"),
        "Empty WS",
        "empty-ws",
        1,
    );
    state.workspaces.create(&ws2).await.unwrap();
    let Json(empty) =
        get_workspace_dependency_graph(State(state.clone()), Path("ws-2".to_string()))
            .await
            .unwrap();
    assert!(empty.edges.is_empty());
    assert!(empty.nodes.is_empty());

    // An unknown workspace 404s.
    match get_workspace_dependency_graph(State(state.clone()), Path("nope".to_string())).await {
        Err(e) => assert_eq!(e.to_string(), "workspace nope not found"),
        Ok(_) => panic!("unknown workspace must return NotFound"),
    }

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
