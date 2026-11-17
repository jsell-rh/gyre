//! Budget cascade integration tests (hierarchy-enforcement.md §7:
//! "tests/hierarchy_cascade.rs — budget cascade: workspace budget can't
//! exceed tenant, repo can't exceed workspace").
//!
//! End-to-end version of the unit test
//! `cascade_validation_rejects_workspace_exceeding_tenant`
//! (api/budget.rs): the real HTTP endpoints over a real SQLite temp-file
//! database (no mem mocks), driven through the full middleware stack.
//!
//! Cascade under test (agent-runtime.md §4 / platform-model.md §1):
//!   tenant ceiling ("tenant:global") ≥ workspace budget ≥ repo budget,
//! with an unset workspace dimension INHERITED from the tenant rather
//! than unlimited. Enforcement points exercised:
//!   - `PUT /api/v1/workspaces/:id/budget` rejects a workspace limit
//!     above the tenant ceiling (400) and accepts one below it.
//!   - `PUT /api/v1/repos/:id` rejects a repo budget above the
//!     workspace's EFFECTIVE limit (min of workspace config and tenant
//!     ceiling) and accepts one below it.
//!   - A workspace with no configured budget inherits the tenant
//!     ceiling: a repo override above the tenant ceiling is rejected
//!     even though the workspace row names no limit of its own.

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Repository};
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "hierarchy-cascade-integration-token";

/// Serializes env-var mutation across tests in this binary (own process).
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

type Guard = tokio::sync::MutexGuard<'static, ()>;

async fn sqlite_ctx() -> (Guard, tempfile::TempDir, Arc<AppState>, axum::Router) {
    let guard = ENV_LOCK.lock().await;
    let dir = tempfile::tempdir().expect("temp db dir");
    let url = format!("sqlite://{}", dir.path().join("gyre.db").display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };
    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in policies");
    let app = build_router(Arc::clone(&state));
    (guard, dir, state, app)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn call(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {TOKEN}"));
    let body = match body {
        Some(b) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(&b).unwrap())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

/// Set the tenant-level ceiling (the "tenant:global" budget config the
/// cascade validates against).
async fn set_tenant_ceiling(state: &AppState, max_tokens_per_day: u64) {
    state
        .budget_configs
        .set_config(
            "tenant:global",
            &BudgetConfig {
                max_tokens_per_day: Some(max_tokens_per_day),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

/// Create a workspace through the real API (global token = Admin, the
/// only role allowed to manage budgets) and a repo row under it.
async fn workspace_with_repo(state: &AppState, app: &axum::Router, name: &str) -> (Id, Id) {
    let resp = call(
        app,
        Method::POST,
        "/api/v1/workspaces",
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let repo = Id::new(format!("cascade-repo-{name}"));
    state
        .repos
        .create(&Repository::new(
            repo.clone(),
            ws.clone(),
            format!("cascade {name}"),
            format!("/tmp/hierarchy-cascade/{name}.git"),
            1000,
        ))
        .await
        .unwrap();

    (ws, repo)
}

async fn put_workspace_budget(app: &axum::Router, ws: &Id, max_tokens: u64) -> StatusCode {
    let resp = call(
        app,
        Method::PUT,
        &format!("/api/v1/workspaces/{}/budget", ws),
        Some(serde_json::json!({ "max_tokens_per_day": max_tokens })),
    )
    .await;
    resp.status()
}

async fn put_repo_budget(app: &axum::Router, repo: &Id, max_tokens: u64) -> StatusCode {
    let resp = call(
        app,
        Method::PUT,
        &format!("/api/v1/repos/{}", repo),
        Some(serde_json::json!({ "budget": { "max_tokens_per_day": max_tokens } })),
    )
    .await;
    resp.status()
}

/// Workspace budget cannot exceed the tenant limit; a lower one is
/// accepted. End-to-end `cascade_validation_rejects_workspace_exceeding_tenant`
/// semantics: same route, same 400, but over the real router + SQLite
/// budget config store instead of a hand-built state.
#[tokio::test]
async fn workspace_budget_cannot_exceed_tenant_limit() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    set_tenant_ceiling(&state, 1000).await;
    let (ws, _repo) = workspace_with_repo(&state, &app, "WsCascade").await;

    assert_eq!(
        put_workspace_budget(&app, &ws, 999_999).await,
        StatusCode::BAD_REQUEST,
        "workspace budget above the tenant ceiling must be rejected"
    );

    assert_eq!(
        put_workspace_budget(&app, &ws, 500).await,
        StatusCode::OK,
        "workspace budget within the tenant ceiling must be accepted"
    );
}

/// Repo budget cannot exceed the workspace limit (which is itself the
/// tighter of the workspace config and the tenant ceiling).
#[tokio::test]
async fn repo_budget_cannot_exceed_workspace_limit() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    set_tenant_ceiling(&state, 1000).await;
    let (ws, repo) = workspace_with_repo(&state, &app, "RepoCascade").await;

    // Workspace limit: 500 (below the 1000 tenant ceiling).
    assert_eq!(put_workspace_budget(&app, &ws, 500).await, StatusCode::OK);

    // Repo above the workspace limit: rejected.
    assert_eq!(
        put_repo_budget(&app, &repo, 999_999).await,
        StatusCode::BAD_REQUEST,
        "repo budget above the workspace limit must be rejected"
    );

    // Repo within the workspace limit: accepted.
    assert_eq!(
        put_repo_budget(&app, &repo, 400).await,
        StatusCode::OK,
        "repo budget within the workspace limit must be accepted"
    );
}

/// An unset workspace dimension is INHERITED from the tenant ceiling, not
/// unlimited: with no workspace budget configured, a repo override above
/// the tenant ceiling is still rejected.
#[tokio::test]
async fn repo_budget_inherits_tenant_ceiling_when_workspace_unset() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    set_tenant_ceiling(&state, 1000).await;
    let (_ws, repo) = workspace_with_repo(&state, &app, "InheritCascade").await;

    assert_eq!(
        put_repo_budget(&app, &repo, 2000).await,
        StatusCode::BAD_REQUEST,
        "repo budget above the inherited tenant ceiling must be rejected"
    );
    assert_eq!(
        put_repo_budget(&app, &repo, 1000).await,
        StatusCode::OK,
        "repo budget at the inherited tenant ceiling must be accepted"
    );
}

/// The cascade is live, not just write-time: lowering the tenant ceiling
/// below an existing workspace budget keeps the effective limit bounded
/// by the new ceiling (effective = min(workspace, tenant)), so a repo
/// override between the old workspace limit and the new tenant ceiling
/// is rejected.
#[tokio::test]
async fn lowered_tenant_ceiling_binds_repos_immediately() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    set_tenant_ceiling(&state, 1000).await;
    let (ws, repo) = workspace_with_repo(&state, &app, "LowerCascade").await;
    assert_eq!(put_workspace_budget(&app, &ws, 800).await, StatusCode::OK);

    // Lower the tenant ceiling below the workspace's configured limit.
    set_tenant_ceiling(&state, 300).await;

    assert_eq!(
        put_repo_budget(&app, &repo, 500).await,
        StatusCode::BAD_REQUEST,
        "a repo override above the lowered tenant ceiling must be rejected \
         even though the workspace config still says 800"
    );
    assert_eq!(
        put_repo_budget(&app, &repo, 300).await,
        StatusCode::OK,
        "a repo override at the lowered tenant ceiling must be accepted"
    );
}
