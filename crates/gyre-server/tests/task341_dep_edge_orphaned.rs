//! task-341: `DependencyStatus::Orphaned` means "Target repo was deleted or
//! archived" (dependency-graph.md §Dependency Entity) — the transition must
//! actually happen in production code, not only in the enum declaration.
//!
//! Storage-backed probes over a real SQLite database (`build_state` with
//! `GYRE_DATABASE_URL=sqlite://<tempdir>` — the dependency port is wired to
//! the Diesel SQLite `DependencyRepository`, no mem stand-in):
//!   - archiving the target through the archive_repo HTTP handler
//!     transitions every dependents' edge to Orphaned, visible through the
//!     dependency port AND GET /api/v1/deps/repo/{B}/dependents, and the
//!     edge row still exists afterwards (never deleted);
//!   - deleting the (archived) target leaves the edges targeting it present
//!     in storage with status Orphaned — including an edge stranded Active
//!     by a pre-fix deletion, which delete_repo itself repairs;
//!   - dep_staleness_check (the continuous monitor) transitions edges whose
//!     target repo row no longer resolves (deleted) or is archived to
//!     Orphaned.
//!
//! The push-side probes (no edge created targeting an archived repo, no
//! resurrection of an Orphaned edge on re-push, re-activation after
//! unarchive) live in git_http.rs
//! (`push_does_not_target_archived_repos_and_unarchive_reactivates`), which
//! drives the real git HTTP receive-pack path end to end.
//!
//! `GYRE_DATABASE_URL` is process-global; these tests serialize on a shared
//! lock and each integration test file runs in its own process.

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{
    DependencyEdge, DependencyStatus, DependencyType, DetectionMethod, Repository, Workspace,
};
use gyre_server::{build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "task341-dep-orphaned-token";

/// Serializes env-var mutation across tests in this binary (own process).
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Build a SQLite-backed AppState over a fresh temp database. The boot
/// tenant is created explicitly (`bootstrap_default_tenant` runs in main.rs
/// at startup, not in `build_state`), and the built-in ABAC policies are
/// seeded so the ABAC-registered /deps routes evaluate for the system token.
async fn sqlite_state() -> (tempfile::TempDir, Arc<AppState>) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("task341.db");
    unsafe {
        std::env::set_var(
            "GYRE_DATABASE_URL",
            format!("sqlite://{}", db_path.display()),
        );
    }
    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    state
        .tenants
        .create(&gyre_domain::Tenant::new(
            Id::new("default"),
            "Default",
            "default",
            1000,
        ))
        .await
        .unwrap();
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in policies");
    (dir, state)
}

async fn make_workspace(state: &Arc<AppState>, id: &str) -> Id {
    let ws = Workspace::new(Id::new(id), Id::new("default"), id, id, 1000);
    state.workspaces.create(&ws).await.unwrap();
    ws.id
}

async fn make_repo(state: &Arc<AppState>, id: &str, name: &str, ws: &Id) -> Repository {
    let repo = Repository::new(
        Id::new(id),
        ws.clone(),
        name,
        format!("/tmp/{id}.git"),
        1000,
    );
    state.repos.create(&repo).await.unwrap();
    repo
}

fn active_edge(id: &str, source: &Id, target: &Id) -> DependencyEdge {
    DependencyEdge::new(
        Id::new(id),
        source.clone(),
        target.clone(),
        DependencyType::Code,
        "Cargo.toml",
        "target-crate",
        DetectionMethod::CargoToml,
        1000,
    )
}

async fn call(app: &axum::Router, method: Method, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Archiving the target repo through the archive_repo handler transitions
/// every stored edge targeting it to Orphaned — visible via the dependency
/// port and GET /api/v1/deps/repo/{B}/dependents — and the edge row still
/// exists afterwards. Pre-fix this fails: archive_repo never touched
/// dependency edges.
#[tokio::test]
async fn archiving_target_repo_orphans_dependent_edges() {
    let _env = ENV_LOCK.lock().await;
    let (_dir, state) = sqlite_state().await;
    let app = build_router(Arc::clone(&state));

    let ws = make_workspace(&state, "ws-t341-arch").await;
    let repo_a = make_repo(&state, "repo-t341-a", "repo-a", &ws).await;
    let repo_b = make_repo(&state, "repo-t341-b", "repo-b", &ws).await;

    let edge = active_edge("edge-t341-arch", &repo_a.id, &repo_b.id);
    state.dependencies.save(&edge).await.unwrap();
    // Sanity: this is the real SQLite DependencyRepository — the row
    // round-trips through the port before any handler runs.
    let stored = state
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .expect("edge persisted through the SQLite dependency port");
    assert_eq!(stored.status, DependencyStatus::Active);

    // Archive B through the production handler.
    let resp = call(
        &app,
        Method::POST,
        &format!("/api/v1/repos/{}/archive", repo_b.id.as_str()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // The edge transitions to Orphaned, visible via the dependency port...
    let dependents = state
        .dependencies
        .list_dependents(&repo_b.id)
        .await
        .unwrap();
    assert_eq!(dependents.len(), 1, "the edge row still exists");
    assert_eq!(dependents[0].id, edge.id);
    assert_eq!(dependents[0].status, DependencyStatus::Orphaned);
    // ...and the row itself survives (never deleted).
    let stored = state
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .expect("edge row survives the archive");
    assert_eq!(stored.status, DependencyStatus::Orphaned);

    // Visible via GET /api/v1/deps/repo/{B}/dependents.
    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/deps/repo/{}/dependents", repo_b.id.as_str()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["status"], "orphaned");
    assert_eq!(arr[0]["source_repo_id"], repo_a.id.as_str());
    assert_eq!(arr[0]["target_repo_id"], repo_b.id.as_str());

    std::env::remove_var("GYRE_DATABASE_URL");
}

/// Deleting the (archived) target repo leaves the edges targeting it present
/// in storage with status Orphaned — they are not deleted with the repo.
/// The edge is reset to Active after the archive to prove delete_repo itself
/// performs the orphaning (a pre-fix deletion leaves exactly this state: the
/// repo row gone, the edge still Active).
#[tokio::test]
async fn deleting_target_repo_keeps_edges_orphaned() {
    let _env = ENV_LOCK.lock().await;
    let (_dir, state) = sqlite_state().await;
    let app = build_router(Arc::clone(&state));

    let ws = make_workspace(&state, "ws-t341-del").await;
    let repo_a = make_repo(&state, "repo-t341-da", "repo-da", &ws).await;
    let repo_b = make_repo(&state, "repo-t341-db", "repo-db", &ws).await;

    let edge = active_edge("edge-t341-del", &repo_a.id, &repo_b.id);
    state.dependencies.save(&edge).await.unwrap();

    // Delete requires the archived state; archive through the handler.
    let resp = call(
        &app,
        Method::POST,
        &format!("/api/v1/repos/{}/archive", repo_b.id.as_str()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Reset the edge to Active: the state a pre-fix deletion strands (the
    // fix must orphan it at delete time, not rely on the archive path).
    let mut stranded = state
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .unwrap();
    stranded.status = DependencyStatus::Active;
    state.dependencies.save(&stranded).await.unwrap();

    let resp = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/repos/{}", repo_b.id.as_str()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // The repo row is gone...
    assert!(state.repos.find_by_id(&repo_b.id).await.unwrap().is_none());
    // ...but the edges targeting it survive the deletion, Orphaned.
    let dependents = state
        .dependencies
        .list_dependents(&repo_b.id)
        .await
        .unwrap();
    assert_eq!(dependents.len(), 1, "edges are not deleted with the repo");
    assert_eq!(dependents[0].id, edge.id);
    assert_eq!(dependents[0].status, DependencyStatus::Orphaned);
    let stored = state
        .dependencies
        .find_by_id(&edge.id)
        .await
        .unwrap()
        .expect("edge row survives the repo deletion");
    assert_eq!(stored.status, DependencyStatus::Orphaned);

    std::env::remove_var("GYRE_DATABASE_URL");
}

/// dep_staleness_check (the continuous monitor) transitions edges whose
/// target repo row no longer resolves in the repo list (deleted — including
/// edges stranded by pre-fix deletions) or is archived to Orphaned. Pre-fix
/// this fails: the job only skipped already-Orphaned edges and never
/// orphaned anything (an unresolvable target yielded target_version None
/// with no status change).
#[tokio::test]
async fn dep_staleness_orphans_edges_with_unresolvable_or_archived_target() {
    let _env = ENV_LOCK.lock().await;
    let (_dir, state) = sqlite_state().await;

    let ws = make_workspace(&state, "ws-t341-mon").await;
    let repo_s = make_repo(&state, "repo-t341-ms", "repo-ms", &ws).await;
    let repo_t1 = make_repo(&state, "repo-t341-mt1", "repo-mt1", &ws).await;
    let repo_t2 = make_repo(&state, "repo-t341-mt2", "repo-mt2", &ws).await;

    // Edge 1: target repo row no longer resolves (deleted).
    let edge1 = active_edge("edge-t341-mon1", &repo_s.id, &repo_t1.id);
    state.dependencies.save(&edge1).await.unwrap();
    state.repos.delete(&repo_t1.id).await.unwrap();

    // Edge 2: target repo is archived (a pre-fix archive leaves exactly
    // this state — the edge was never touched by the archival).
    let edge2 = active_edge("edge-t341-mon2", &repo_s.id, &repo_t2.id);
    state.dependencies.save(&edge2).await.unwrap();
    let mut archived = repo_t2.clone();
    archived.archive();
    state.repos.update(&archived).await.unwrap();

    gyre_server::dep_staleness::run_once(&state).await.unwrap();

    let updated1 = state
        .dependencies
        .find_by_id(&edge1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated1.status,
        DependencyStatus::Orphaned,
        "edge whose target repo row no longer resolves must be orphaned"
    );
    let updated2 = state
        .dependencies
        .find_by_id(&edge2.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated2.status,
        DependencyStatus::Orphaned,
        "edge whose target repo is archived must be orphaned"
    );

    std::env::remove_var("GYRE_DATABASE_URL");
}
