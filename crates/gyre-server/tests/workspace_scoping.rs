//! Workspace scoping at the HTTP layer (hierarchy-enforcement.md §7:
//! "tests/workspace_scoping.rs — entities created in workspace A are not
//! visible in workspace B's scoped routes").
//!
//! Two workspaces in the SAME tenant (the boot tenant "default"), each
//! populated with a repo, task, agent, and merge request. User principals
//! (real API keys over real SQLite — no mem mocks, no listener needed:
//! requests ride the full `require_auth` → ABAC middleware stack via
//! `tower::ServiceExt::oneshot`) verify:
//!
//!   - ALLOW when policy conditions are met: a member of workspace A
//!     lists A's scoped routes and sees ONLY A's entities.
//!   - DENY when they are not: the same caller addressing workspace B's
//!     scoped routes (or B's entities by id) gets 403 — the builtin
//!     `workspace-membership-required` Deny (940) fires on the real
//!     membership store, above the Developer role Allow (800).
//!   - A same-tenant caller with NO membership is denied too: workspace
//!     isolation is membership-scoped, not merely tenant-scoped.
//!
//! The workspace rows are created through the real API by their future
//! members, so the creator's Owner membership is seeded by the production
//! path (`create_workspace`), not by the test.

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{Agent, MergeRequest, Repository, Task};
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "workspace-scoping-integration-token";

/// Serializes env-var mutation across tests in this binary (own process).
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

type Guard = tokio::sync::MutexGuard<'static, ()>;

async fn sqlite_ctx() -> (Guard, tempfile::TempDir, Arc<AppState>, axum::Router) {
    let guard = ENV_LOCK.lock().await;
    let dir = tempfile::tempdir().expect("temp db dir");
    let url = format!("sqlite://{}", dir.path().join("gyre.db").display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };
    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    // The boot tenant ("default") — `bootstrap_default_tenant` runs in
    // main.rs at startup, not in `build_state`, so a fresh test DB needs it
    // before users can be provisioned into it.
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
    token: &str,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"));
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

async fn make_user(app: &axum::Router, username: &str, roles: &[&str]) -> String {
    let resp = call(
        app,
        Method::POST,
        "/api/v1/users",
        TOKEN,
        Some(serde_json::json!({
            "username": username,
            "tenant_id": "default",
            "roles": roles,
        })),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "user provisioning failed"
    );
    body_json(resp).await["api_key"]["key"]
        .as_str()
        .expect("api key")
        .to_string()
}

struct WsEntities {
    workspace: Id,
    repo: Id,
    task: Id,
    agent: Id,
    mr: Id,
}

async fn seed_entities(state: &AppState, ws: &Id, tag: &str) -> WsEntities {
    let repo = Id::new(format!("repo-{tag}"));
    let task = Id::new(format!("task-{tag}"));
    let agent = Id::new(format!("agent-{tag}"));
    let mr = Id::new(format!("mr-{tag}"));

    state
        .repos
        .create(&Repository::new(
            repo.clone(),
            ws.clone(),
            format!("repo {tag}"),
            format!("/tmp/workspace-scoping/{tag}.git"),
            1000,
        ))
        .await
        .unwrap();

    let mut t = Task::new(task.clone(), format!("task {tag}"), 1000);
    t.workspace_id = ws.clone();
    t.repo_id = repo.clone();
    state.tasks.create(&t).await.unwrap();

    let mut a = Agent::new(agent.clone(), format!("agent {tag}"), 1000);
    a.workspace_id = ws.clone();
    state.agents.create(&a).await.unwrap();

    state
        .merge_requests
        .create(&MergeRequest::new(
            mr.clone(),
            repo.clone(),
            ws.clone(),
            format!("mr {tag}"),
            format!("feature/{tag}"),
            "main",
            1000,
        ))
        .await
        .unwrap();

    WsEntities {
        workspace: ws.clone(),
        repo,
        task,
        agent,
        mr,
    }
}

/// Two workspaces (same tenant) owned by two different Developer users,
/// each populated with one repo/task/agent/MR.
async fn two_workspace_world() -> (
    Guard,
    tempfile::TempDir,
    Arc<AppState>,
    axum::Router,
    String,
    String,
    WsEntities,
    WsEntities,
) {
    let (guard, dir, state, app) = sqlite_ctx().await;

    let key_a = make_user(&app, "ws-a-owner", &["Developer"]).await;
    let key_b = make_user(&app, "ws-b-owner", &["Developer"]).await;

    let resp = call(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        &key_a,
        Some(serde_json::json!({ "name": "Scope Alpha" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws_a = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let resp = call(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        &key_b,
        Some(serde_json::json!({ "name": "Scope Beta" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws_b = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let a = seed_entities(&state, &ws_a, "a").await;
    let b = seed_entities(&state, &ws_b, "b").await;

    (guard, dir, state, app, key_a, key_b, a, b)
}

/// Member of A lists A's scoped routes → 200 with ONLY A's entities.
#[tokio::test]
async fn workspace_a_member_sees_only_workspace_a_entities() {
    let (_guard, _dir, _state, app, key_a, _key_b, a, b) = two_workspace_world().await;

    for (label, path, expected) in [
        ("tasks", "tasks", a.task.as_str()),
        ("agents", "agents", a.agent.as_str()),
        ("merge-requests", "merge-requests", a.mr.as_str()),
        ("repos", "repos", a.repo.as_str()),
    ] {
        let resp = call(
            &app,
            Method::GET,
            &format!("/api/v1/workspaces/{}/{}", a.workspace, path),
            &key_a,
            None,
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "member must list own workspace's {label}"
        );
        let j = body_json(resp).await;
        let arr = j.as_array().expect("entity list");
        let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
        assert_eq!(
            ids,
            vec![expected],
            "workspace A's {label} listing must contain exactly A's entity"
        );
        assert!(
            !ids.contains(&b.task.as_str())
                && !ids.contains(&b.agent.as_str())
                && !ids.contains(&b.mr.as_str())
                && !ids.contains(&b.repo.as_str()),
            "workspace A's {label} listing leaked workspace B entities"
        );
    }
}

/// Member of A addressing workspace B's scoped routes → 403 (membership
/// Deny at 940 outranks the Developer role Allow at 800).
#[tokio::test]
async fn workspace_a_member_cannot_list_workspace_b_scoped_routes() {
    let (_guard, _dir, _state, app, key_a, _key_b, _a, b) = two_workspace_world().await;

    for path in ["tasks", "agents", "merge-requests", "repos"] {
        let resp = call(
            &app,
            Method::GET,
            &format!("/api/v1/workspaces/{}/{}", b.workspace, path),
            &key_a,
            None,
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "workspace A member must be denied workspace B's {path} listing"
        );
        let j = body_json(resp).await;
        assert_eq!(j["error"].as_str().unwrap(), "insufficient permissions");
    }
}

/// Member of A addressing workspace B's entities by id → 403; own → 200.
#[tokio::test]
async fn addressed_entities_are_workspace_scoped() {
    let (_guard, _dir, _state, app, key_a, _key_b, a, b) = two_workspace_world().await;

    for (label, uri) in [
        ("task", format!("/api/v1/tasks/{}", b.task)),
        ("agent", format!("/api/v1/agents/{}", b.agent)),
        ("merge request", format!("/api/v1/merge-requests/{}", b.mr)),
        ("repo", format!("/api/v1/repos/{}", b.repo)),
    ] {
        let resp = call(&app, Method::GET, &uri, &key_a, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "workspace A member must be denied workspace B's {label} by id"
        );
    }

    for (label, uri) in [
        ("task", format!("/api/v1/tasks/{}", a.task)),
        ("agent", format!("/api/v1/agents/{}", a.agent)),
        ("merge request", format!("/api/v1/merge-requests/{}", a.mr)),
        ("repo", format!("/api/v1/repos/{}", a.repo)),
    ] {
        let resp = call(&app, Method::GET, &uri, &key_a, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "workspace A member must read own {label} by id"
        );
    }
}

/// Writes are scoped like reads: a member of A cannot create entities in
/// workspace B through its scoped route.
#[tokio::test]
async fn workspace_a_member_cannot_write_into_workspace_b() {
    let (_guard, _dir, _state, app, key_a, _key_b, _a, b) = two_workspace_world().await;

    let resp = call(
        &app,
        Method::POST,
        &format!("/api/v1/workspaces/{}/tasks", b.workspace),
        &key_a,
        Some(serde_json::json!({ "title": "intruder task" })),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "workspace A member must not create tasks in workspace B"
    );
    let j = body_json(resp).await;
    assert_eq!(j["error"].as_str().unwrap(), "insufficient permissions");
}

/// Same-tenant caller with NO membership in either workspace is denied
/// both: workspace isolation is membership-scoped, not tenant-scoped.
#[tokio::test]
async fn membershipless_same_tenant_caller_is_denied() {
    let (_guard, _dir, _state, app, _key_a, _key_b, a, _b) = two_workspace_world().await;

    let key_outsider = make_user(&app, "ws-outsider", &["Developer"]).await;

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", a.workspace),
        &key_outsider,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "a same-tenant non-member must not list a workspace's scoped routes"
    );

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/tasks/{}", a.task),
        &key_outsider,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "a same-tenant non-member must not read a workspace's entities by id"
    );
}

/// The workspace row itself is scoped: A's member reads A's workspace, is
/// denied B's.
#[tokio::test]
async fn workspace_rows_are_scoped_to_membership() {
    let (_guard, _dir, _state, app, key_a, _key_b, a, b) = two_workspace_world().await;

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}", a.workspace),
        &key_a,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}", b.workspace),
        &key_a,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "workspace A member must be denied workspace B's row"
    );
}
