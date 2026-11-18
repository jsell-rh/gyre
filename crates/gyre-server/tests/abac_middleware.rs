//! ABAC middleware integration tests (hierarchy-enforcement.md §7:
//! "tests/abac_middleware.rs — ABAC middleware denies access when policy
//! conditions aren't met; allows when they are").
//!
//! Drives the REAL middleware stack (`require_auth` → `abac_middleware` →
//! handler) over a real SQLite temp-file database (no mem mocks) with real
//! principals: API-key users (Developer / ReadOnly), a legacy per-agent
//! UUID token (the `agent_tokens` KV path issued at agent registration),
//! and the global system token. Requests go through `build_router` via
//! `tower::ServiceExt::oneshot` — no listener needed, full production
//! layering.
//!
//! Deny/allow pairs asserted against the seeded builtin policy set:
//!   - `admin-only-deny` (850) + default-deny: an Agent-role token on an
//!     admin resource is denied; the Admin global token is allowed.
//!   - `readonly-get-only` (600): a ReadOnly member may read a
//!     workspace-scoped route but is denied writes (no Allow matches the
//!     `write` action → default-deny).
//!   - `workspace-membership-required` (940) vs `developer-write-access`
//!     (800): a Developer member is allowed on their workspace's scoped
//!     route; a membership-less same-tenant Developer is denied.
//!   - `agent-scoped-access` (700): an Agent-role token may read platform
//!     resources in its scope.
//!   - Denials use the spec's opaque 403 shape
//!     (`{"error": "insufficient permissions"}`, §Middleware Design).

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{Agent, Repository, Task, WorkspaceMembership, WorkspaceRole};
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "abac-middleware-integration-token";

/// Serializes env-var mutation across tests in this binary (own process).
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

type Guard = tokio::sync::MutexGuard<'static, ()>;

async fn sqlite_ctx() -> (Guard, tempfile::TempDir, Arc<AppState>, axum::Router) {
    let guard = ENV_LOCK.lock().await;
    let dir = tempfile::tempdir().expect("temp db dir");
    let url = format!("sqlite://{}", dir.path().join("gyre.db").display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };
    let state = build_state(TOKEN, "http://127.0.0.1:1", None);
    // The boot tenant — `bootstrap_default_tenant` runs in main.rs at
    // startup, not in `build_state`, so a fresh test DB needs it before
    // users can be provisioned into it.
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

/// No Authorization header at all.
async fn call_no_auth(app: &axum::Router, method: Method, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Provision a user + API key through the real admin API. Returns
/// (user_id, raw_api_key).
async fn make_user(app: &axum::Router, username: &str, roles: &[&str]) -> (String, String) {
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
    let j = body_json(resp).await;
    (
        j["user"]["id"].as_str().expect("user id").to_string(),
        j["api_key"]["key"].as_str().expect("api key").to_string(),
    )
}

/// A workspace created through the real API by `key`'s user (the creator
/// becomes its Owner), plus one repo and one task under it.
async fn owned_workspace(state: &AppState, app: &axum::Router, key: &str) -> (Id, Id) {
    let resp = call(
        app,
        Method::POST,
        "/api/v1/workspaces",
        key,
        Some(serde_json::json!({ "name": "Abac Fixture" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let repo = Id::new("abac-repo");
    state
        .repos
        .create(&Repository::new(
            repo.clone(),
            ws.clone(),
            "fixture",
            "/tmp/abac.git",
            1000,
        ))
        .await
        .unwrap();

    let task = Id::new("abac-task");
    let mut t = Task::new(task.clone(), "fixture task", 1000);
    t.workspace_id = ws.clone();
    t.repo_id = repo.clone();
    state.tasks.create(&t).await.unwrap();

    (ws, task)
}

/// A legacy per-agent UUID token (the `agent_tokens` KV path): an agent
/// row in `ws` plus the token the auth extractor resolves to Agent role.
async fn agent_uuid_token(state: &AppState, ws: &Id) -> (String, String) {
    let agent_id = Id::new("abac-agent");
    let mut agent = Agent::new(agent_id.clone(), "abac fixture agent", 1000);
    agent.workspace_id = ws.clone();
    state.agents.create(&agent).await.unwrap();

    let token = format!("agent-token-{}", uuid::Uuid::new_v4());
    state
        .kv_store
        .kv_set("agent_tokens", agent_id.as_str(), token.clone())
        .await
        .unwrap();
    (agent_id.to_string(), token)
}

// ── Admin-resource band: Agent denied, Admin allowed ─────────────────────────

#[tokio::test]
async fn agent_token_denied_on_admin_route_admin_token_allowed() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    let (ws, _task) = owned_workspace(&state, &app, TOKEN).await;
    let (_agent_id, agent_token) = agent_uuid_token(&state, &ws).await;

    // Deny: the Agent role has no Allow that covers the "admin" resource
    // type (admin-only-deny 850 matches unconditionally on it).
    let resp = call(
        &app,
        Method::GET,
        "/api/v1/admin/health",
        &agent_token,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "agent token must be denied on admin routes"
    );
    let j = body_json(resp).await;
    assert_eq!(
        j["error"].as_str().unwrap(),
        "insufficient permissions",
        "ABAC denial must use the opaque 403 shape (§Middleware Design)"
    );

    // Allow: the global token carries the Admin role
    // (admin-all-operations 900).
    let resp = call(&app, Method::GET, "/api/v1/admin/health", TOKEN, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "admin token must pass the same route the agent was denied on"
    );
}

// ── Role band: ReadOnly may read, may not write ──────────────────────────────

#[tokio::test]
async fn readonly_member_reads_but_cannot_write() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    let (dev_user, dev_key) = make_user(&app, "abac-dev", &["Developer"]).await;
    let (ro_user, ro_key) = make_user(&app, "abac-readonly", &["ReadOnly"]).await;
    let (ws, _task) = owned_workspace(&state, &app, &dev_key).await;

    // Seed the ReadOnly user's Viewer membership (the membership store the
    // middleware reads; a ReadOnly caller cannot create workspaces, so the
    // Owner path is not available — same seeding as the production
    // invitation-accept flow writes).
    let mut membership = WorkspaceMembership::new(
        Id::new("abac-ro-membership"),
        Id::new(ro_user),
        ws.clone(),
        WorkspaceRole::Viewer,
        Id::new(dev_user),
        1000,
    );
    membership.accept(1000);
    state
        .workspace_memberships
        .create(&membership)
        .await
        .unwrap();

    // Allow: read on a workspace-scoped route (readonly-get-only 600,
    // membership satisfied).
    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", ws),
        &ro_key,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "ReadOnly member must be allowed to read the scoped route"
    );

    // Deny: write on the same route — no Allow policy matches the `write`
    // action for the ReadOnly role, so the default-deny catches it.
    let resp = call(
        &app,
        Method::POST,
        &format!("/api/v1/workspaces/{}/tasks", ws),
        &ro_key,
        Some(serde_json::json!({ "title": "should be denied" })),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "ReadOnly member must be denied writes on the scoped route"
    );
    let j = body_json(resp).await;
    assert_eq!(j["error"].as_str().unwrap(), "insufficient permissions");
}

// ── Membership band: Developer member allowed, membership-less denied ────────

#[tokio::test]
async fn developer_membership_decides_scoped_route_access() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    let (_dev_user, dev_key) = make_user(&app, "abac-dev2", &["Developer"]).await;
    let (_other_user, other_key) = make_user(&app, "abac-dev3", &["Developer"]).await;
    let (ws, _task) = owned_workspace(&state, &app, &dev_key).await;

    // Allow: the workspace's Owner (a Developer) reads the scoped route
    // (developer-write-access 800, membership-required 940 satisfied).
    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", ws),
        &dev_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Deny: same role, same tenant, but no membership — the builtin
    // workspace-membership-required (940) outranks the role Allow (800).
    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", ws),
        &other_key,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "membership-less Developer must be denied the scoped route"
    );
    let j = body_json(resp).await;
    assert_eq!(j["error"].as_str().unwrap(), "insufficient permissions");
}

// ── Agent read band: agent token reads in-scope platform resources ───────────

#[tokio::test]
async fn agent_token_reads_platform_resources() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    let (ws, task) = owned_workspace(&state, &app, TOKEN).await;
    let (_agent_id, agent_token) = agent_uuid_token(&state, &ws).await;

    // Allow: agent-scoped-access (700) permits the read action; the
    // workspace resolves inside the agent token's tenant scope, so
    // tenant-isolation (950) does not match.
    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/tasks/{}", task),
        &agent_token,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "agent token must read an in-scope task"
    );
}

// ── Boundary: the auth layer under the ABAC layer ────────────────────────────

#[tokio::test]
async fn unauthenticated_request_rejected_before_abac() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;
    let resp = call_no_auth(&app, Method::GET, "/api/v1/workspaces").await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "missing bearer token must be rejected by require_auth before ABAC"
    );
}
