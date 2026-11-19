//! SCIM user-lifecycle integration tests (task-265,
//! identity-security.md §SCIM Provisioning).
//!
//! Every request goes through the FULL production `build_router()` stack
//! (`require_auth_middleware` → `last_seen_middleware` → `abac_middleware`
//! → handler) over a real SQLite temp-file database — never a directly
//! mounted `api_router()`. That is the point: the pre-task wiring defect
//! (the global auth middleware rejecting SCIM bearer tokens 401 before
//! `check_scim_auth` could run) was invisible to tests that bypassed the
//! middleware stack, so these tests prove the wiring end-to-end:
//!
//! - a request bearing ONLY `Authorization: Bearer $GYRE_SCIM_TOKEN`
//!   reaches the SCIM handlers and succeeds;
//! - a wrong token, a missing header, or an unset GYRE_SCIM_TOKEN is
//!   rejected (the exemption did not open an unauthenticated hole);
//! - PATCH `active:false` (RFC 7644 PatchOp and the simplified bare-body
//!   form) is a real lifecycle event: stored record deactivated, all
//!   sessions revoked (a pre-deactivation API key replays 401), agent
//!   `spawned_by` attribution cleared;
//! - SCIM DELETE deprovisions (soft deactivation) — the row remains
//!   resolvable, never a hard store delete;
//! - the enterprise extension (RFC 7643 §4.3) round-trips through
//!   create/PUT/PATCH;
//! - userName/externalId stay immutable and ServiceProviderConfig
//!   advertises PATCH support.
//!
//! This binary runs in its own process, so GYRE_SCIM_TOKEN manipulation
//! here cannot race the in-process unit tests of other binaries; within
//! the binary, ENV_LOCK serializes env mutation with in-flight requests.

use axum::body::Body;
use gyre_common::Id;
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "scim-lifecycle-integration-token";
const SCIM_TOKEN: &str = "scim-lifecycle-bearer";

static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

type Guard = tokio::sync::MutexGuard<'static, ()>;

async fn sqlite_ctx() -> (Guard, tempfile::TempDir, Arc<AppState>, axum::Router) {
    let guard = ENV_LOCK.lock().await;
    let dir = tempfile::tempdir().expect("temp db dir");
    let url = format!("sqlite://{}", dir.path().join("gyre.db").display());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };
    unsafe { std::env::set_var("GYRE_SCIM_TOKEN", SCIM_TOKEN) };
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
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Request through the full production stack with the SCIM bearer token
/// as the ONLY credential (no global token, no JWT).
async fn scim_call(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {SCIM_TOKEN}"));
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

/// Request with an arbitrary Authorization header value (or none).
async fn raw_call(
    app: &axum::Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(a) = auth {
        builder = builder.header("Authorization", a.to_string());
    }
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

/// Provision a user + API key through the real admin API (global token).
async fn make_user(app: &axum::Router, username: &str) -> (String, String) {
    let resp = raw_call(
        app,
        Method::POST,
        "/api/v1/users",
        Some(&format!("Bearer {TOKEN}")),
        Some(serde_json::json!({
            "username": username,
            "tenant_id": "default",
            "roles": ["Developer"],
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

/// An agent attributed to `spawned_by` (the attribution the deactivation
/// cascade must clear).
async fn agent_spawned_by(state: &AppState, spawned_by: &str, n: u8) -> Id {
    let id = Id::new(format!("scim-agent-{n}"));
    let mut agent = gyre_domain::Agent::new(id.clone(), format!("scim-agent-{n}"), 1000);
    agent.spawned_by = Some(spawned_by.to_string());
    state.agents.create(&agent).await.unwrap();
    id
}

/// The deactivation lifecycle effects shared by PATCH active:false and
/// SCIM DELETE: stored record inactive, every session revoked, agent
/// spawned_by attribution cleared, pre-deactivation credential dead.
async fn assert_deactivated(
    state: &AppState,
    app: &axum::Router,
    user_id: &str,
    api_key: &str,
    agent_id: &Id,
) {
    // Stored record: soft-deactivated, still resolvable (no hard delete).
    let stored = state
        .users
        .find_by_id(&Id::new(user_id.to_string()))
        .await
        .unwrap()
        .expect("deactivated user row must remain in the store");
    assert!(
        stored.is_deactivated(),
        "SCIM lifecycle must deactivate the stored record"
    );

    // Serialized SCIM resource reports the stored state.
    let resp = scim_call(app, Method::GET, &format!("/scim/v2/Users/{user_id}"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_json(resp).await["active"],
        serde_json::Value::Bool(false),
        "serialized active must mirror the stored deactivation"
    );

    // Every session for the user is revoked.
    let sessions = state
        .sessions
        .list_for_user(&Id::new(user_id.to_string()))
        .await
        .unwrap();
    assert!(
        !sessions.is_empty(),
        "control: the pre-deactivation request must have minted a session row"
    );
    assert!(
        sessions.iter().all(|s| s.revoked),
        "revoke_all_for_user must have revoked every session"
    );

    // The pre-deactivation credential no longer authenticates (401, not
    // 403: the auth extractor rejects revoked-session credentials).
    let resp = raw_call(
        app,
        Method::GET,
        "/api/v1/users/me",
        Some(&format!("Bearer {api_key}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a pre-deactivation session credential must be rejected after deactivation"
    );

    // Agent spawned_by attribution cleared.
    let agent = state
        .agents
        .find_by_id(agent_id)
        .await
        .unwrap()
        .expect("agent must remain");
    assert_eq!(
        agent.spawned_by, None,
        "deactivation must remove the user from agents' spawned_by tracking"
    );
}

// ── Wiring: the SCIM bearer token through the production stack ──────────────

#[tokio::test]
async fn scim_bearer_token_reaches_handlers_through_production_stack() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;

    // A request bearing ONLY the SCIM bearer token succeeds — pre-task,
    // require_auth_middleware rejected this 401 ("Invalid token") before
    // check_scim_auth ever ran.
    let resp = scim_call(&app, Method::GET, "/scim/v2/Users", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "SCIM bearer token must pass the global auth middleware"
    );

    // And a full create round-trip with only the SCIM token.
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Users",
        Some(serde_json::json!({ "userName": "wired-user" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    assert_eq!(created["userName"], "wired-user");

    // Discovery endpoints too (same middleware stack).
    let resp = scim_call(&app, Method::GET, "/scim/v2/ServiceProviderConfig", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn scim_wrong_or_missing_token_rejected() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;

    // Wrong bearer value: the handler-level check rejects 403.
    let resp = raw_call(
        &app,
        Method::GET,
        "/scim/v2/Users",
        Some("Bearer definitely-not-the-scim-token"),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Missing Authorization header entirely.
    let resp = raw_call(&app, Method::GET, "/scim/v2/Users", None, None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // A valid GLOBAL token is not a SCIM credential either: the SCIM
    // surface accepts only the dedicated SCIM token.
    let resp = raw_call(
        &app,
        Method::GET,
        "/scim/v2/Users",
        Some(&format!("Bearer {TOKEN}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "the global API token must not authenticate SCIM requests"
    );
}

#[tokio::test]
async fn scim_unset_token_fails_closed() {
    let (guard, _dir, _state, app) = sqlite_ctx().await;

    // With GYRE_SCIM_TOKEN unset, every /scim/v2 request must be rejected
    // 403 — the middleware exemption must not open an unauthenticated hole
    // when the dedicated token is not configured.
    unsafe { std::env::remove_var("GYRE_SCIM_TOKEN") };
    let resp = raw_call(
        &app,
        Method::GET,
        "/scim/v2/Users",
        Some("Bearer anything"),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "unset GYRE_SCIM_TOKEN must fail closed"
    );
    // Restore for any subsequent holder of the lock.
    unsafe { std::env::set_var("GYRE_SCIM_TOKEN", SCIM_TOKEN) };
    drop(guard);
}

/// Review repair (task-265, round 2): a successful SCIM request must not
/// leave a false AuthFailure audit row behind. Pre-repair,
/// `last_seen_middleware` — layered over the SCIM routes in
/// `build_router` — ran the `AuthenticatedAgent` extractor with the SCIM
/// bearer token; the ladder rejected it and its final fallback persisted
/// an AuthFailure ('invalid token', CEF severity 6 in the SIEM export) for
/// every legitimate IdP provisioning call, masking real credential
/// attacks. The audit channel must stay quiet for authorized SCIM traffic.
#[tokio::test]
async fn successful_scim_requests_do_not_record_auth_failures() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    // A spread of successful SCIM calls through the full production
    // stack (list, create, PATCH, GET).
    let resp = scim_call(&app, Method::GET, "/scim/v2/Users", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Users",
        Some(serde_json::json!({ "userName": "audit-clean-user" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{id}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "replace", "path": "displayName", "value": "Clean" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = scim_call(&app, Method::GET, &format!("/scim/v2/Users/{id}"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Zero AuthFailure rows: authorized provisioning traffic must not
    // pollute the audit trail.
    let failures = state
        .audit
        .query(&gyre_ports::AuditQueryFilter {
            event_type: Some("auth_failure".to_string()),
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        failures.is_empty(),
        "successful SCIM requests must not record AuthFailure audit events, \
         found {}",
        failures.len()
    );

    // Control — the audit channel is alive: a genuinely invalid bearer on
    // an /api/v1 route (rejected by require_auth_middleware, which owns
    // its AuthFailure emission) records exactly one TRUE row, so the zero
    // above is not a vacuous pass.
    let resp = raw_call(
        &app,
        Method::GET,
        "/api/v1/users/me",
        Some("Bearer definitely-not-any-credential"),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let failures = state
        .audit
        .query(&gyre_ports::AuditQueryFilter {
            event_type: Some("auth_failure".to_string()),
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        failures.len(),
        1,
        "exactly the one true failure — no false rows from the SCIM calls above"
    );
}

// ── Deactivation lifecycle ───────────────────────────────────────────────────

#[tokio::test]
async fn patch_active_false_deactivates_user_end_to_end() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    let (user_id, api_key) = make_user(&app, "patch-victim").await;

    // Mint a live session for the user's API key (an authenticated
    // request tracks the session) and an agent attributed to the user.
    let resp = raw_call(
        &app,
        Method::GET,
        "/api/v1/users/me",
        Some(&format!("Bearer {api_key}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "control: credential live pre-deactivation"
    );
    let agent_id = agent_spawned_by(&state, &user_id, 1).await;

    // RFC 7644 PatchOp: replace active with false.
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{user_id}"),
        Some(serde_json::json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{ "op": "replace", "path": "active", "value": false }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_json(resp).await["active"],
        serde_json::Value::Bool(false)
    );

    assert_deactivated(&state, &app, &user_id, &api_key, &agent_id).await;
}

#[tokio::test]
async fn patch_simplified_body_deactivates_user() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    let (user_id, api_key) = make_user(&app, "patch-simple").await;

    let resp = raw_call(
        &app,
        Method::GET,
        "/api/v1/users/me",
        Some(&format!("Bearer {api_key}")),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let agent_id = agent_spawned_by(&state, &user_id, 2).await;

    // The simplified bare-body form (no Operations array).
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{user_id}"),
        Some(serde_json::json!({ "active": false })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_json(resp).await["active"],
        serde_json::Value::Bool(false)
    );

    assert_deactivated(&state, &app, &user_id, &api_key, &agent_id).await;
}

#[tokio::test]
async fn patch_profile_fields_without_deactivating() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;

    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Users",
        Some(serde_json::json!({ "userName": "patch-profile", "displayName": "Before" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    // A PATCH that does not touch `active` must not deactivate (omitted
    // attributes are unchanged, not "unset to default").
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{id}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "replace", "path": "displayName", "value": "After" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(patched["displayName"], "After");
    assert_eq!(patched["active"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn scim_delete_deprovisions_without_hard_delete() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;
    let (user_id, api_key) = make_user(&app, "delete-victim").await;

    let resp = raw_call(
        &app,
        Method::GET,
        "/api/v1/users/me",
        Some(&format!("Bearer {api_key}")),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let agent_id = agent_spawned_by(&state, &user_id, 3).await;

    let resp = scim_call(
        &app,
        Method::DELETE,
        &format!("/scim/v2/Users/{user_id}"),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Deprovision = the same lifecycle cascade as active:false, with the
    // row remaining resolvable (asserted inside, including GET 200).
    assert_deactivated(&state, &app, &user_id, &api_key, &agent_id).await;
}

// ── Enterprise extension (RFC 7643 §4.3) ─────────────────────────────────────

#[tokio::test]
async fn enterprise_extension_serialized_and_persisted() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;
    let (manager_id, _manager_key) = make_user(&app, "the-manager").await;

    // Create carries the enterprise extension.
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Users",
        Some(serde_json::json!({
            "userName": "enterprise-user",
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": {
                "organization": "default",
                "manager": { "value": manager_id }
            }
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap().to_string();

    let schemas = created["schemas"].as_array().unwrap();
    assert!(schemas
        .iter()
        .any(|s| s == "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"));
    let ext = &created["urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"];
    assert_eq!(ext["organization"], "default");
    assert_eq!(ext["manager"]["value"], manager_id);

    // PUT accepts and persists the extension (manager change).
    let resp = scim_call(
        &app,
        Method::PUT,
        &format!("/scim/v2/Users/{id}"),
        Some(serde_json::json!({
            "userName": "enterprise-user",
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": {
                "manager": { "value": id }
            }
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp).await;
    assert_eq!(
        updated["urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"]["manager"]["value"],
        id
    );

    // And the persisted value survives a fresh GET.
    let resp = scim_call(&app, Method::GET, &format!("/scim/v2/Users/{id}"), None).await;
    let got = body_json(resp).await;
    assert_eq!(
        got["urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"]["manager"]["value"],
        id
    );

    // PATCH (path form) updates the manager too.
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{id}"),
        Some(serde_json::json!({
            "Operations": [{
                "op": "replace",
                "path": "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager",
                "value": { "value": manager_id }
            }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(
        patched["urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"]["manager"]["value"],
        manager_id
    );
}

// ── Immutability + capability advertisement ─────────────────────────────────

#[tokio::test]
async fn patch_cannot_rename_user_name_or_external_id() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;

    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Users",
        Some(serde_json::json!({
            "userName": "immutable-handle",
            "externalId": "ext-immutable"
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Users/{id}"),
        Some(serde_json::json!({
            "Operations": [
                { "op": "replace", "path": "userName", "value": "renamed" },
                { "op": "replace", "path": "externalId", "value": "swapped-identity" }
            ]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(patched["userName"], "immutable-handle");
    assert_eq!(patched["externalId"], "ext-immutable");

    // ServiceProviderConfig must advertise PATCH now that it is implemented.
    let resp = scim_call(&app, Method::GET, "/scim/v2/ServiceProviderConfig", None).await;
    let cfg = body_json(resp).await;
    assert_eq!(cfg["patch"]["supported"], serde_json::Value::Bool(true));
}
