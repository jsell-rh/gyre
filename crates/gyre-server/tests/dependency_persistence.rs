//! Integration test: the cross-repo dependency graph is persistent forge state
//! (dependency-graph.md §Dependency Entity).
//!
//! Proves that with `GYRE_DATABASE_URL` pointing at a SQLite file,
//! `build_state` wires `AppState.dependencies` to the persistent
//! `DependencyRepository` (SqliteStorage), so dependency edges survive a
//! restart — a second `build_state` over the same DB file sees the edges
//! written by the first.
//!
//! This test lives in its own test binary (own process) because
//! `build_state` reads `GYRE_DATABASE_URL` from the process environment;
//! other integration binaries must not see the variable.
//!
//! Against the old wiring (`Arc::new(mem::MemDependencyRepository::default())`
//! hardcoded in `build_state`) the second instance sees an empty store and
//! this test fails.

use gyre_common::Id;
use gyre_domain::{DependencyEdge, DependencyStatus, DependencyType, DetectionMethod};
use gyre_server::build_state;
use tempfile::TempDir;

fn sample_edge(id: &str, source: &str, target: &str) -> DependencyEdge {
    DependencyEdge::new(
        Id::new(id),
        Id::new(source),
        Id::new(target),
        DependencyType::Code,
        "Cargo.toml",
        "gyre-common",
        DetectionMethod::CargoToml,
        1_700_000_000,
    )
}

/// Round-trip one edge through two genuinely fresh `build_state` instances
/// over the same SQLite file: save via instance 1, drop it, build instance 2,
/// and require the edge to be visible (find_by_id + list_by_repo + list_all).
#[tokio::test(flavor = "multi_thread")]
async fn dependency_graph_survives_restart_on_sqlite() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("gyre.db");
    let db_url = format!("sqlite://{}", db_path.display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &db_url) };

    let source = Id::new("dep-persist-source");
    let target = Id::new("dep-persist-target");
    let edge = sample_edge("dep-persist-edge-1", source.as_str(), target.as_str());

    // Instance 1: write the edge, then prove it is visible in this instance.
    {
        let state = build_state("dep-persist-token", "http://127.0.0.1:1", None);
        state.dependencies.save(&edge).await.unwrap();
        let seen = state.dependencies.find_by_id(&edge.id).await.unwrap();
        assert!(
            seen.is_some(),
            "edge must be readable from the instance that wrote it"
        );
    }

    // Instance 2: a genuinely fresh AppState + fresh SqliteStorage pool over
    // the same file — a server restart. The mem adapter would see nothing.
    let state2 = build_state("dep-persist-token", "http://127.0.0.1:1", None);

    let found = state2
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .expect("edge must survive restart on SQLite-backed state");
    assert_eq!(found.source_repo_id, source);
    assert_eq!(found.target_repo_id, target);
    assert_eq!(found.dependency_type, DependencyType::Code);
    assert_eq!(found.source_artifact, "Cargo.toml");
    assert_eq!(found.target_artifact, "gyre-common");
    assert_eq!(found.detection_method, DetectionMethod::CargoToml);
    assert_eq!(found.status, DependencyStatus::Active);
    assert_eq!(found.detected_at, 1_700_000_000);

    let outgoing = state2.dependencies.list_by_repo(&source).await.unwrap();
    assert_eq!(
        outgoing.len(),
        1,
        "restart must preserve outgoing edges for the source repo"
    );
    assert_eq!(outgoing[0].id, edge.id);

    let incoming = state2.dependencies.list_dependents(&target).await.unwrap();
    assert_eq!(
        incoming.len(),
        1,
        "restart must preserve incoming edges for the target repo"
    );

    let all = state2.dependencies.list_all().await.unwrap();
    assert!(
        all.iter().any(|e| e.id == edge.id),
        "restart must preserve the edge in the tenant-wide graph"
    );

    // Write-through on the second instance must persist too (update path).
    let mut updated = found;
    updated.status = DependencyStatus::Stale;
    updated.version_pinned = Some("1.2.3".to_string());
    state2.dependencies.save(&updated).await.unwrap();
    drop(state2);

    // Instance 3: confirm the update round-trips as well.
    let state3 = build_state("dep-persist-token", "http://127.0.0.1:1", None);
    let reloaded = state3
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .expect("updated edge must survive second restart");
    assert_eq!(reloaded.status, DependencyStatus::Stale);
    assert_eq!(reloaded.version_pinned.as_deref(), Some("1.2.3"));

    unsafe { std::env::remove_var("GYRE_DATABASE_URL") };
}
