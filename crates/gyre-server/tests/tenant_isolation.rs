//! Cross-tenant isolation at the HTTP layer (hierarchy-enforcement.md §7:
//! "tests/tenant_isolation.rs — two-tenant scenario: listing/finding
//! through tenant A never leaks tenant B data").
//!
//! Complements `crates/gyre-adapters/tests/tenant_isolation.rs` (task-160),
//! which proves the STORAGE-level filter: two `SqliteStorage` handles over
//! one DB file, differing only by tenant scope, never return each other's
//! rows. This file proves the SERVER-level isolation that rides on top of
//! that storage: real `build_state` over a real SQLite temp-file database
//! (no mem mocks), real API-key principals bound to each tenant, and the
//! full middleware stack (`require_auth` → ABAC) deciding every request.
//!
//! What is actually load-bearing at this layer, per request path:
//!   - `GET /api/v1/workspaces` — handler scopes `list_by_tenant` to the
//!     caller's verified tenant binding (auth extractor), so tenant A's
//!     listing never contains tenant B's workspaces, and vice versa.
//!   - Addressed entity reads (`/repos/:id`, `/tasks/:id`, `/agents/:id`,
//!     `/merge-requests/:id`, `/workspaces/:id`) — the ABAC middleware
//!     resolves the entity → workspace scope chain and denies
//!     cross-tenant/non-member access (builtin `tenant-isolation` 950 /
//!     `workspace-membership-required` 940) BEFORE the handler runs, so a
//!     tenant A caller gets 403, never tenant B's entity body.
//!
//! Deployment note (single shared storage handle): the server opens ONE
//! `SqliteStorage` scoped to the boot tenant ("default"); entity rows
//! (repos/tasks/agents/MRs) created through it carry that storage stamp,
//! while workspace rows carry their real tenant. Cross-tenant denial on
//! addressed routes is therefore enforced by the ABAC scope chain above —
//! which is exactly what this file asserts. The storage-handle isolation
//! for arbitrary tenant scopes is the adapters test's job.

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{Agent, MergeRequest, Repository, Task, Tenant};
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "tenant-isolation-integration-token";
const TENANT_A: &str = "default";
const TENANT_B: &str = "tenant-beta";

/// Serializes env-var mutation across tests in this binary (each test file
/// is its own process). Same pattern as `search_wiring.rs`.
static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

type Guard = tokio::sync::MutexGuard<'static, ()>;

/// Real SQLite state on a fresh temp file. Pokes `GYRE_DATABASE_URL` while
/// holding `ENV_LOCK`; the guard is returned so the env var stays owned by
/// this test until it finishes.
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
        .create(&Tenant::new(Id::new("default"), "Default", "default", 1000))
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

/// Provision a user + API key through the real admin API (global token is
/// the system principal: it may create users in any tenant). Returns
/// (user_id, raw_api_key).
async fn make_user(
    app: &axum::Router,
    username: &str,
    tenant: &str,
    roles: &[&str],
) -> (String, String) {
    let resp = call(
        app,
        Method::POST,
        "/api/v1/users",
        TOKEN,
        Some(serde_json::json!({
            "username": username,
            "tenant_id": tenant,
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
    let user_id = j["user"]["id"].as_str().expect("user id").to_string();
    let key = j["api_key"]["key"].as_str().expect("api key").to_string();
    (user_id, key)
}

/// Entities seeded under one tenant's workspace: repo, task, agent, MR.
struct TenantEntities {
    workspace: Id,
    repo: Id,
    task: Id,
    agent: Id,
    mr: Id,
}

async fn seed_entities(state: &AppState, ws: &Id, tag: &str) -> TenantEntities {
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
            format!("/tmp/tenant-isolation/{tag}.git"),
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

    TenantEntities {
        workspace: ws.clone(),
        repo,
        task,
        agent,
        mr,
    }
}

/// Full two-tenant fixture: tenants, Developer users with API keys bound
/// to each tenant, one workspace per tenant (created through the real API
/// so the creator's Owner membership is seeded), and entities under each.
async fn two_tenant_world() -> (
    Guard,
    tempfile::TempDir,
    Arc<AppState>,
    axum::Router,
    String,
    String,
    TenantEntities,
    TenantEntities,
) {
    let (guard, dir, state, app) = sqlite_ctx().await;

    // Tenant B must exist before users can be bound to it. Tenant A is the
    // boot tenant ("default") the storage handle is scoped to.
    state
        .tenants
        .create(&Tenant::new(Id::new(TENANT_B), "Beta", "beta", 1000))
        .await
        .unwrap();

    let (_user_a, key_a) = make_user(&app, "tenant-a-dev", TENANT_A, &["Developer"]).await;
    let (_user_b, key_b) = make_user(&app, "tenant-b-dev", TENANT_B, &["Developer"]).await;

    // Each user creates their own workspace through the real API: the
    // handler derives the workspace's tenant from the creator's verified
    // binding (non-admin callers cannot pick a tenant) and seeds the
    // creator's Owner membership.
    let resp = call(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        &key_a,
        Some(serde_json::json!({ "name": "Alpha" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws_a = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let resp = call(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        &key_b,
        Some(serde_json::json!({ "name": "Beta" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws_b = Id::new(body_json(resp).await["id"].as_str().unwrap().to_string());

    let a = seed_entities(&state, &ws_a, "a").await;
    let b = seed_entities(&state, &ws_b, "b").await;

    (guard, dir, state, app, key_a, key_b, a, b)
}

// ── Listing: tenant A's listing never contains tenant B's workspaces ─────────

#[tokio::test]
async fn workspace_listing_is_scoped_to_the_callers_tenant() {
    let (_guard, _dir, _state, app, key_a, key_b, a, b) = two_tenant_world().await;

    let resp = call(&app, Method::GET, "/api/v1/workspaces", &key_a, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let j = body_json(resp).await;
    let arr = j.as_array().expect("workspace list");
    let ids: Vec<&str> = arr.iter().filter_map(|w| w["id"].as_str()).collect();
    assert!(
        ids.contains(&a.workspace.as_str()),
        "tenant A must see its own workspace"
    );
    assert!(
        !ids.contains(&b.workspace.as_str()),
        "tenant A's workspace listing leaked tenant B's workspace"
    );
    // Every returned row carries the caller's tenant scope.
    for w in arr {
        assert_eq!(w["tenant_id"].as_str().unwrap(), TENANT_A);
    }

    // And symmetrically for tenant B.
    let resp = call(&app, Method::GET, "/api/v1/workspaces", &key_b, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let j = body_json(resp).await;
    let arr = j.as_array().expect("workspace list");
    let ids: Vec<&str> = arr.iter().filter_map(|w| w["id"].as_str()).collect();
    assert!(ids.contains(&b.workspace.as_str()));
    assert!(
        !ids.contains(&a.workspace.as_str()),
        "tenant B's workspace listing leaked tenant A's workspace"
    );
    for w in arr {
        assert_eq!(w["tenant_id"].as_str().unwrap(), TENANT_B);
    }
}

// ── Finding: addressed entities of tenant B are denied to tenant A ───────────

#[tokio::test]
async fn tenant_a_cannot_read_tenant_b_entities() {
    let (_guard, _dir, _state, app, key_a, _key_b, _a, b) = two_tenant_world().await;

    for (label, uri) in [
        ("repo", format!("/api/v1/repos/{}", b.repo)),
        ("task", format!("/api/v1/tasks/{}", b.task)),
        ("agent", format!("/api/v1/agents/{}", b.agent)),
        ("merge request", format!("/api/v1/merge-requests/{}", b.mr)),
        ("workspace", format!("/api/v1/workspaces/{}", b.workspace)),
    ] {
        let resp = call(&app, Method::GET, &uri, &key_a, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "tenant A must be denied tenant B's {label} ({uri}), got {}",
            resp.status()
        );
        // Never a body leak either: the denial is the middleware's
        // opaque 403, not the entity.
        let j = body_json(resp).await;
        assert_eq!(
            j["error"].as_str().unwrap(),
            "insufficient permissions",
            "cross-tenant denial must be the ABAC 403 shape"
        );
    }
}

#[tokio::test]
async fn tenant_a_reads_own_entities() {
    // The denial above must be scope-driven, not a blanket lockout: the
    // same tenant A caller reads its OWN entities fine.
    let (_guard, _dir, _state, app, key_a, _key_b, a, _b) = two_tenant_world().await;

    for (label, uri) in [
        ("repo", format!("/api/v1/repos/{}", a.repo)),
        ("task", format!("/api/v1/tasks/{}", a.task)),
        ("agent", format!("/api/v1/agents/{}", a.agent)),
        ("merge request", format!("/api/v1/merge-requests/{}", a.mr)),
        ("workspace", format!("/api/v1/workspaces/{}", a.workspace)),
    ] {
        let resp = call(&app, Method::GET, &uri, &key_a, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "tenant A must read its own {label} ({uri}), got {}",
            resp.status()
        );
    }
}

#[tokio::test]
async fn tenant_b_reads_own_entities_but_not_tenant_a() {
    // Symmetric check from tenant B's side.
    let (_guard, _dir, _state, app, _key_a, key_b, a, b) = two_tenant_world().await;

    for (label, uri) in [
        ("repo", format!("/api/v1/repos/{}", b.repo)),
        ("task", format!("/api/v1/tasks/{}", b.task)),
        ("agent", format!("/api/v1/agents/{}", b.agent)),
        ("merge request", format!("/api/v1/merge-requests/{}", b.mr)),
    ] {
        let resp = call(&app, Method::GET, &uri, &key_b, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "tenant B must read its own {label} ({uri}), got {}",
            resp.status()
        );
    }

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/tasks/{}", a.task),
        &key_b,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "tenant B must be denied tenant A's task"
    );
}

#[tokio::test]
async fn workspace_scoped_listing_never_crosses_tenants() {
    // The workspace-scoped collection routes (`/workspaces/:id/...`) are
    // the primary access pattern (api-conventions.md §1.1): a tenant A
    // member listing tenant B's workspace-scoped collections is denied,
    // and their own listing returns only their own entities.
    let (_guard, _dir, _state, app, key_a, _key_b, a, b) = two_tenant_world().await;

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", b.workspace),
        &key_a,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "tenant A must not list tenant B's workspace-scoped tasks"
    );

    let resp = call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{}/tasks", a.workspace),
        &key_a,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let j = body_json(resp).await;
    let arr = j.as_array().expect("task list");
    let ids: Vec<&str> = arr.iter().filter_map(|t| t["id"].as_str()).collect();
    assert_eq!(ids, vec![a.task.as_str()], "own listing must be exact");
}
