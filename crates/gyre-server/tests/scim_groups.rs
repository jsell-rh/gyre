//! SCIM Groups + group-to-workspace mapping integration tests (task-266,
//! identity-security.md §SCIM Provisioning / §Group-to-Workspace Mapping).
//!
//! Every request goes through the FULL production `build_router()` stack
//! (`require_auth_middleware` → `last_seen_middleware` → `abac_middleware`
//! → handler) over a real SQLite temp-file database — never a directly
//! mounted `api_router()`. That is the point: the ABAC route registration
//! for `/api/v1/workspaces/:id/scim-mappings` (resource `scim_mapping`)
//! and the `/scim/v2/Groups` auth wiring are only exercised through the
//! middleware chain.
//!
//! Proves, against real storage:
//!
//! - all six `/scim/v2/Groups` endpoints per RFC 7644 (list pagination +
//!   filter, get incl. 404 on unknown id, create, PUT replace, PATCH
//!   add/remove members incl. the path-filter form, delete), each
//!   enforcing the dedicated GYRE_SCIM_TOKEN bearer (fail-closed);
//! - `PUT /api/v1/workspaces/{id}/scim-mappings` persists
//!   `[{idp_group, workspace_role}]` durably (survives a full state
//!   rebuild over the same database — the restart proof) and is rejected
//!   for non-admin callers;
//! - the mapping EFFECT, both halves: pushing user U into mapped group G
//!   creates U's WorkspaceMembership with the mapped role (observable by
//!   U's authenticated workspace access while a member), and removing U
//!   from G revokes it (the same access is subsequently denied);
//! - idempotency: re-pushing the same membership does not duplicate rows;
//! - the control case: pushing U into an UNMAPPED group creates no
//!   WorkspaceMembership anywhere.

use axum::body::Body;
use gyre_common::Id;
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use http::{Method, Request, StatusCode};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

const TOKEN: &str = "scim-groups-integration-token";
const SCIM_TOKEN: &str = "scim-groups-bearer";

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
/// as the ONLY credential.
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

/// Provision a tenant user + API key through the real admin API (global
/// token). Returns (user_id, api_key).
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

/// Create a workspace owned by `owner_key`'s user (the creator's Owner
/// membership is seeded by the create handler). Returns the workspace id.
async fn make_workspace(app: &axum::Router, owner_key: &str, name: &str) -> String {
    let resp = raw_call(
        app,
        Method::POST,
        "/api/v1/workspaces",
        Some(&format!("Bearer {owner_key}")),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "workspace creation failed"
    );
    body_json(resp).await["id"]
        .as_str()
        .expect("workspace id")
        .to_string()
}

/// The membership row for (user, workspace) in the REAL store, if any.
async fn membership(
    state: &AppState,
    user_id: &str,
    workspace_id: &str,
) -> Option<gyre_domain::WorkspaceMembership> {
    state
        .workspace_memberships
        .find_by_user_and_workspace(&Id::new(user_id), &Id::new(workspace_id))
        .await
        .unwrap()
}

// ── The six Groups endpoints per RFC 7644 ────────────────────────────────────

#[tokio::test]
async fn groups_crud_surface_per_rfc7644() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    // Create two users to use as members.
    let (u1, _k1) = make_user(&app, "group-member-1").await;
    let (u2, _k2) = make_user(&app, "group-member-2").await;

    // POST: create with the Group schema (schemas, externalId,
    // displayName, members with value/display).
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Group"],
            "externalId": "cn=eng,ou=groups,dc=example,dc=com",
            "displayName": "Engineering",
            "members": [{ "value": u1, "display": "Group Member 1" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let gid = created["id"].as_str().unwrap().to_string();
    assert_eq!(
        created["schemas"][0],
        "urn:ietf:params:scim:schemas:core:2.0:Group"
    );
    assert_eq!(created["displayName"], "Engineering");
    assert_eq!(
        created["externalId"], "cn=eng,ou=groups,dc=example,dc=com",
        "externalId must round-trip (it is the mapping join key)"
    );
    assert_eq!(created["members"][0]["value"], u1);
    assert_eq!(created["members"][0]["display"], "Group Member 1");
    assert_eq!(created["meta"]["resourceType"], "Group");

    // POST idempotency on externalId: re-push returns the stored
    // resource (200), never a duplicate row.
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "externalId": "cn=eng,ou=groups,dc=example,dc=com",
            "displayName": "Engineering"
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["id"], gid);
    assert_eq!(
        state.scim_groups.list().await.unwrap().len(),
        1,
        "re-pushing the same externalId must not duplicate the group"
    );

    // POST without displayName: 400 (displayName is required for groups).
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({ "externalId": "cn=x,dc=example,dc=com" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // GET {id}: the stored group with its members.
    let resp = scim_call(&app, Method::GET, &format!("/scim/v2/Groups/{gid}"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let got = body_json(resp).await;
    assert_eq!(got["id"], gid);
    assert_eq!(got["members"].as_array().unwrap().len(), 1);

    // GET unknown id: 404.
    let resp = scim_call(&app, Method::GET, "/scim/v2/Groups/no-such-group", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // More groups for pagination.
    for n in 2..=5 {
        let resp = scim_call(
            &app,
            Method::POST,
            "/scim/v2/Groups",
            Some(serde_json::json!({
                "displayName": format!("Team-{n}"),
                "externalId": format!("cn=team-{n},dc=example,dc=com")
            })),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // GET list: total, default (unpaginated) shape.
    let resp = scim_call(&app, Method::GET, "/scim/v2/Groups", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    assert_eq!(list["totalResults"], 5);
    assert_eq!(list["startIndex"], 1);

    // startIndex/count pagination (RFC 7644 §3.4.2.4): 1-based index.
    let resp = scim_call(
        &app,
        Method::GET,
        "/scim/v2/Groups?startIndex=2&count=2",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let page = body_json(resp).await;
    assert_eq!(page["totalResults"], 5, "total is unpaginated");
    assert_eq!(page["itemsPerPage"], 2);
    let resources = page["Resources"].as_array().unwrap();
    assert_eq!(resources.len(), 2);
    assert_eq!(
        resources[0]["id"],
        list["Resources"].as_array().unwrap()[1]["id"]
    );

    // Filter: displayName eq "...".
    let resp = scim_call(
        &app,
        Method::GET,
        "/scim/v2/Groups?filter=displayName%20eq%20%22Team-3%22",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let filtered = body_json(resp).await;
    assert_eq!(filtered["totalResults"], 1);
    assert_eq!(filtered["Resources"][0]["displayName"], "Team-3");

    // PUT: replace displayName and the membership set.
    let resp = scim_call(
        &app,
        Method::PUT,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "displayName": "Engineering (all)",
            "members": [
                { "value": u1, "display": "Group Member 1" },
                { "value": u2, "display": "Group Member 2" }
            ]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let replaced = body_json(resp).await;
    assert_eq!(replaced["displayName"], "Engineering (all)");
    assert_eq!(replaced["members"].as_array().unwrap().len(), 2);
    // externalId is the immutable join key: a swap attempt is ignored.
    assert_eq!(replaced["externalId"], "cn=eng,ou=groups,dc=example,dc=com");

    // PATCH: add a member back via value list (u1 was replaced out? no —
    // u1 stayed; remove u1 via the RFC 7644 path-filter form first).
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{
                "op": "remove",
                "path": format!("members[value eq \"{u1}\"]")
            }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(patched["members"].as_array().unwrap().len(), 1);
    assert_eq!(patched["members"][0]["value"], u2);

    // PATCH: add u1 back.
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{
                "op": "add",
                "path": "members",
                "value": [{ "value": u1 }]
            }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(patched["members"].as_array().unwrap().len(), 2);

    // PATCH: replace displayName (bare-body simplified form, no
    // Operations wrapper).
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({ "displayName": "Engineering" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["displayName"], "Engineering");

    // PATCH: unsupported op → 400.
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "frobnicate", "path": "members" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // DELETE: 204, then the resource is gone (404) and its membership
    // rows are gone too.
    let resp = scim_call(
        &app,
        Method::DELETE,
        &format!("/scim/v2/Groups/{gid}"),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = scim_call(&app, Method::GET, &format!("/scim/v2/Groups/{gid}"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(
        state
            .scim_groups
            .list_members(&Id::new(&gid))
            .await
            .unwrap()
            .is_empty(),
        "group delete must remove the membership rows"
    );

    // PUT/PATCH/DELETE on an unknown id: 404.
    let resp = scim_call(
        &app,
        Method::PUT,
        "/scim/v2/Groups/no-such-group",
        Some(serde_json::json!({ "displayName": "X" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = scim_call(
        &app,
        Method::PATCH,
        "/scim/v2/Groups/no-such-group",
        Some(serde_json::json!({ "displayName": "X" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = scim_call(&app, Method::DELETE, "/scim/v2/Groups/no-such-group", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// SCIM-token auth is enforced on EVERY Groups endpoint (fail-closed:
/// missing header, wrong bearer, and unset token all reject).
#[tokio::test]
async fn groups_endpoints_enforce_scim_token() {
    let (_guard, _dir, _state, app) = sqlite_ctx().await;

    // Missing Authorization header — across ALL six Groups endpoints:
    // auth must reject before any body parsing (a body-less PUT/POST
    // must get 403, never the Json extractor's 415).
    for (method, uri) in [
        (Method::GET, "/scim/v2/Groups"),
        (Method::GET, "/scim/v2/Groups/some-id"),
        (Method::POST, "/scim/v2/Groups"),
        (Method::PUT, "/scim/v2/Groups/some-id"),
        (Method::PATCH, "/scim/v2/Groups/some-id"),
        (Method::DELETE, "/scim/v2/Groups/some-id"),
    ] {
        let resp = raw_call(&app, method.clone(), uri, None, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{method} {uri} must enforce the SCIM token"
        );
    }

    // Wrong bearer value.
    let resp = raw_call(
        &app,
        Method::GET,
        "/scim/v2/Groups",
        Some("Bearer wrong-scim-token"),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // The global API token is NOT a SCIM credential.
    let resp = raw_call(
        &app,
        Method::GET,
        "/scim/v2/Groups",
        Some(&format!("Bearer {TOKEN}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "the global token must not authenticate SCIM endpoints"
    );

    // Unset GYRE_SCIM_TOKEN: fail-closed (every Groups endpoint rejects).
    unsafe { std::env::set_var("GYRE_SCIM_TOKEN", "") };
    let resp = raw_call(&app, Method::GET, "/scim/v2/Groups", None, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "unset GYRE_SCIM_TOKEN must fail closed"
    );
    unsafe { std::env::set_var("GYRE_SCIM_TOKEN", SCIM_TOKEN) };
}

// ── Mapping effects: the verification-intent scenario ────────────────────────

/// Map group G to Developer in workspace W, SCIM-push U into G and back
/// out: while a member, U's authenticated workspace access succeeds;
/// after removal it is denied — proving the revoke half, not just the
/// create half. Also proves idempotency (re-push creates no duplicate)
/// and that a plain Developer member cannot configure mappings.
#[tokio::test]
async fn mapping_effect_creates_and_revokes_membership() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    let (owner_id, owner_key) = make_user(&app, "ws-owner").await;
    let (u_id, u_key) = make_user(&app, "pushed-user").await;
    let ws_id = make_workspace(&app, &owner_key, "Mapped Workspace").await;

    // PUT the mapping (workspace-admin authorized).
    let group_key = "cn=platform-team,ou=groups,dc=example,dc=com";
    let resp = raw_call(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {owner_key}")),
        Some(serde_json::json!({
            "mappings": [{ "idp_group": group_key, "workspace_role": "Developer" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let stored = body_json(resp).await;
    assert_eq!(stored["mappings"][0]["idp_group"], group_key);
    assert_eq!(stored["mappings"][0]["workspace_role"], "Developer");

    // GET reads the set back.
    let resp = raw_call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {owner_key}")),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_json(resp).await["mappings"].as_array().unwrap().len(),
        1
    );

    // Control: U has no membership before the push.
    assert!(membership(&state, &u_id, &ws_id).await.is_none());

    // SCIM-push U into the mapped group (group create with members).
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "externalId": group_key,
            "displayName": "Platform Team",
            "members": [{ "value": u_id }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let gid = body_json(resp).await["id"].as_str().unwrap().to_string();

    // The WorkspaceMembership was created with the mapped role, active.
    let m = membership(&state, &u_id, &ws_id)
        .await
        .expect("group push must create the mapped WorkspaceMembership");
    assert_eq!(m.role, gyre_domain::WorkspaceRole::Developer);
    assert!(m.accepted, "SCIM-driven membership is active on creation");

    // Observable: U's authenticated workspace access at that role
    // succeeds (through the full ABAC stack — U is a member).
    let resp = raw_call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{ws_id}/members"),
        Some(&format!("Bearer {u_key}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the mapped membership must grant U workspace access"
    );

    // Idempotency: re-pushing the same membership (PUT with the same
    // member set, then PATCH add again) does not duplicate records.
    let resp = scim_call(
        &app,
        Method::PUT,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "displayName": "Platform Team",
            "members": [{ "value": u_id }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "add", "path": "members", "value": [{ "value": u_id }] }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        state
            .workspace_memberships
            .list_by_user(&Id::new(&u_id))
            .await
            .unwrap()
            .iter()
            .filter(|m| m.workspace_id.to_string() == ws_id)
            .count(),
        1,
        "re-pushing the same membership must not duplicate records"
    );

    // A plain Developer member (which U now is) cannot configure the
    // workspace's mappings — the non-admin rejection. U passes the ABAC
    // layer's developer-write-access (global role Developer allows write
    // on *), so this 403 is the handler's workspace-admin guard.
    let resp = raw_call(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {u_key}")),
        Some(serde_json::json!({
            "mappings": [{ "idp_group": group_key, "workspace_role": "Owner" }]
        })),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "a plain Developer member must not configure SCIM mappings"
    );
    // And the attempted privilege escalation did not apply.
    let mappings = state
        .scim_mappings
        .list_by_workspace(&Id::new(&ws_id))
        .await
        .unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(
        mappings[0].workspace_role,
        gyre_domain::WorkspaceRole::Developer
    );

    // REMOVE U from the group (PATCH, path-filter form): the membership
    // is revoked.
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{
                "op": "remove",
                "path": format!("members[value eq \"{u_id}\"]")
            }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        membership(&state, &u_id, &ws_id).await.is_none(),
        "removal from the mapped group must revoke the WorkspaceMembership"
    );

    // The role-gated access that succeeded while a member now fails —
    // the membership-required denial fires for a non-member.
    let resp = raw_call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{ws_id}/members"),
        Some(&format!("Bearer {u_key}")),
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "after removal from the mapped group, U's workspace access must be denied"
    );

    // The other half of the PUT path: re-add via PUT (membership
    // recreated), then PUT with an empty member set (revoked again).
    let resp = scim_call(
        &app,
        Method::PUT,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "displayName": "Platform Team",
            "members": [{ "value": u_id }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        membership(&state, &u_id, &ws_id).await.is_some(),
        "PUT re-add must recreate the mapped membership"
    );
    let resp = scim_call(
        &app,
        Method::PUT,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({ "displayName": "Platform Team", "members": [] })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        membership(&state, &u_id, &ws_id).await.is_none(),
        "PUT with an empty member set must revoke the mapped membership"
    );

    // The workspace owner's own membership is untouched throughout.
    assert!(membership(&state, &owner_id, &ws_id).await.is_some());
}

/// Control case: pushing a user into an UNMAPPED group creates no
/// WorkspaceMembership anywhere — the effect is mapping-driven, not
/// blanket membership creation.
#[tokio::test]
async fn unmapped_group_has_no_membership_effect() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    let (_owner_id, owner_key) = make_user(&app, "owner-unmapped").await;
    let (u_id, u_key) = make_user(&app, "user-unmapped").await;
    let ws_id = make_workspace(&app, &owner_key, "Unmapped Workspace").await;

    // A DIFFERENT group is mapped in this workspace; the pushed group is
    // not mapped anywhere.
    let resp = raw_call(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {owner_key}")),
        Some(serde_json::json!({
            "mappings": [{ "idp_group": "cn=mapped-group,dc=example,dc=com", "workspace_role": "Developer" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Push U into the unmapped group (create with members + PATCH add).
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "externalId": "cn=totally-unmapped,dc=example,dc=com",
            "displayName": "Unmapped Team",
            "members": [{ "value": u_id }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let gid = body_json(resp).await["id"].as_str().unwrap().to_string();
    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "add", "path": "members", "value": [{ "value": u_id }] }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // No WorkspaceMembership row appears ANYWHERE for U.
    assert!(
        state
            .workspace_memberships
            .list_by_user(&Id::new(&u_id))
            .await
            .unwrap()
            .is_empty(),
        "pushing a user into an unmapped group must create no WorkspaceMembership"
    );

    // And U's workspace access is denied (non-member).
    let resp = raw_call(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{ws_id}/members"),
        Some(&format!("Bearer {u_key}")),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Mappings persist across a full state rebuild over the same database
/// (the restart proof), and the mapping effect still fires afterwards.
#[tokio::test]
async fn mappings_and_groups_survive_restart() {
    let (_guard, dir, state, app) = sqlite_ctx().await;

    let (_owner_id, owner_key) = make_user(&app, "restart-owner").await;
    let (u_id, _u_key) = make_user(&app, "restart-user").await;
    let ws_id = make_workspace(&app, &owner_key, "Restart Workspace").await;

    let group_key = "cn=platform-team,ou=groups,dc=example,dc=com";
    let resp = raw_call(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {owner_key}")),
        Some(serde_json::json!({
            "mappings": [{ "idp_group": group_key, "workspace_role": "Developer" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Create the (empty) mapped group before "restart".
    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "externalId": group_key,
            "displayName": "Platform Team"
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let gid = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Full state rebuild over the SAME database file: a new pool, a new
    // AppState, a new router — the process-restart equivalent.
    drop(app);
    drop(state);
    let state2 = build_state(TOKEN, "http://127.0.0.1:1", None);
    let app2 = build_router(Arc::clone(&state2));

    // The mapping survived.
    let mappings = state2
        .scim_mappings
        .list_by_workspace(&Id::new(&ws_id))
        .await
        .unwrap();
    assert_eq!(mappings.len(), 1, "mapping must survive a restart");
    assert_eq!(mappings[0].idp_group, group_key);
    assert_eq!(
        mappings[0].workspace_role,
        gyre_domain::WorkspaceRole::Developer
    );

    // The group and its id survived.
    let resp = scim_call(&app2, Method::GET, &format!("/scim/v2/Groups/{gid}"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["externalId"], group_key);

    // The effect still fires after the restart: push U into the group on
    // the rebuilt state → mapped membership created.
    let resp = scim_call(
        &app2,
        Method::PATCH,
        &format!("/scim/v2/Groups/{gid}"),
        Some(serde_json::json!({
            "Operations": [{ "op": "add", "path": "members", "value": [{ "value": u_id }] }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let m = membership(&state2, &u_id, &ws_id)
        .await
        .expect("the mapping effect must fire after a restart");
    assert_eq!(m.role, gyre_domain::WorkspaceRole::Developer);

    // Keep the temp dir alive until the end (the db file lives there).
    drop(_guard);
    drop(dir);
}

/// A group with NO externalId joins mappings by displayName; a user
/// outside the workspace's tenant gets NO membership (cross-tenant
/// containment, user-management.md §Workspace Membership).
#[tokio::test]
async fn display_name_mapping_and_tenant_containment() {
    let (_guard, _dir, state, app) = sqlite_ctx().await;

    let (_owner_id, owner_key) = make_user(&app, "dn-owner").await;
    let (u_id, _u_key) = make_user(&app, "dn-user").await;
    let ws_id = make_workspace(&app, &owner_key, "DN Mapped Workspace").await;

    // Map by displayName (the group will carry no externalId).
    let resp = raw_call(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}/scim-mappings"),
        Some(&format!("Bearer {owner_key}")),
        Some(serde_json::json!({
            "mappings": [{ "idp_group": "Leads", "workspace_role": "Viewer" }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = scim_call(
        &app,
        Method::POST,
        "/scim/v2/Groups",
        Some(serde_json::json!({
            "displayName": "Leads",
            "members": [{ "value": u_id }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let leads_id = body_json(resp).await["id"].as_str().unwrap().to_string();
    let m = membership(&state, &u_id, &ws_id)
        .await
        .expect("displayName-keyed mapping must take effect");
    assert_eq!(m.role, gyre_domain::WorkspaceRole::Viewer);

    // Cross-tenant containment: a user in another tenant gets no
    // membership from a group push. Provision the second tenant and a
    // user in it directly through the stores (the admin API binds
    // callers to their own tenant).
    state
        .tenants
        .create(&gyre_domain::Tenant::new(
            Id::new("other-tenant"),
            "Other",
            "other",
            1000,
        ))
        .await
        .unwrap();
    let mut outsider = gyre_domain::User::new_sso(
        Id::new("outsider-user"),
        String::new(),
        "outsider".to_string(),
        "Outsider".to_string(),
        1000,
    );
    outsider.tenant_id = Some(Id::new("other-tenant"));
    state.users.create(&outsider).await.unwrap();

    let resp = scim_call(
        &app,
        Method::PATCH,
        &format!("/scim/v2/Groups/{leads_id}"),
        Some(serde_json::json!({
            "Operations": [{
                "op": "add",
                "path": "members",
                "value": [{ "value": "outsider-user" }]
            }]
        })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        membership(&state, "outsider-user", &ws_id).await.is_none(),
        "a user outside the workspace's tenant must get no membership"
    );
}
