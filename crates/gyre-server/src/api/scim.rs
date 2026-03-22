//! SCIM 2.0 (RFC 7643 / RFC 7644) provisioning endpoints.
//!
//! Base path: `/scim/v2`
//! Authentication: `GYRE_SCIM_TOKEN` env var (separate Bearer token from main API token).
//!
//! Implements a subset sufficient for enterprise IdP (Okta, Entra ID, Keycloak) integration:
//! - Users CRUD (create, read, update, delete/deactivate)
//! - ServiceProviderConfig
//! - Schemas + ResourceTypes discovery

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use gyre_common::Id;
use gyre_domain::User;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::error::ApiError;
use super::{new_id, now_secs};
use crate::AppState;

// ── SCIM Auth ─────────────────────────────────────────────────────────────────

fn check_scim_auth(headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = std::env::var("GYRE_SCIM_TOKEN").unwrap_or_default();
    if expected.is_empty() {
        // SCIM token not configured — deny all requests for safety.
        return Err(ApiError::Forbidden(
            "GYRE_SCIM_TOKEN not configured".to_string(),
        ));
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token != expected {
        return Err(ApiError::Forbidden("invalid SCIM token".to_string()));
    }
    Ok(())
}

// ── SCIM Types ────────────────────────────────────────────────────────────────

/// SCIM resource meta block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimMeta {
    pub resource_type: String,
    pub location: String,
    pub created: Option<String>,
    pub last_modified: Option<String>,
}

/// SCIM email value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScimEmail {
    pub value: String,
    pub primary: bool,
    #[serde(rename = "type")]
    pub email_type: String,
}

/// SCIM User resource (RFC 7643 §4.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimUser {
    pub schemas: Vec<String>,
    pub id: String,
    pub external_id: Option<String>,
    pub user_name: String,
    pub display_name: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub emails: Vec<ScimEmail>,
    pub meta: ScimMeta,
}

/// SCIM ListResponse wrapper (RFC 7644 §3.4.2).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimListResponse<T: Serialize> {
    pub schemas: Vec<String>,
    pub total_results: usize,
    pub start_index: u64,
    pub items_per_page: usize,
    pub resources: Vec<T>,
}

/// Request body for POST /scim/v2/Users and PUT /scim/v2/Users/{id}.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimUserRequest {
    pub user_name: String,
    pub display_name: Option<String>,
    pub external_id: Option<String>,
    pub active: Option<bool>,
    #[serde(default)]
    pub emails: Vec<ScimEmail>,
}

/// Query params for GET /scim/v2/Users.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimListQuery {
    pub start_index: Option<u64>,
    pub count: Option<usize>,
    pub filter: Option<String>,
}

// ── Conversions ───────────────────────────────────────────────────────────────

fn user_to_scim(user: &User) -> ScimUser {
    let location = format!("/scim/v2/Users/{}", user.id);
    let created = epoch_to_iso(user.created_at);
    let last_modified = epoch_to_iso(user.updated_at);

    let emails: Vec<ScimEmail> = user
        .email
        .as_ref()
        .map(|e| {
            vec![ScimEmail {
                value: e.clone(),
                primary: true,
                email_type: "work".to_string(),
            }]
        })
        .unwrap_or_default();

    ScimUser {
        schemas: vec!["urn:ietf:params:scim:schemas:core:2.0:User".to_string()],
        id: user.id.to_string(),
        external_id: if user.external_id.is_empty() {
            None
        } else {
            Some(user.external_id.clone())
        },
        user_name: user.username.clone(),
        display_name: Some(user.display_name.clone()),
        active: true, // deactivation tracked via future `active` field on User
        emails,
        meta: ScimMeta {
            resource_type: "User".to_string(),
            location,
            created: Some(created),
            last_modified: Some(last_modified),
        },
    }
}

fn epoch_to_iso(secs: u64) -> String {
    // Simple ISO 8601 UTC format without external dep.
    let total_secs = secs;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = (total_secs / 3600) % 24;
    let days = total_secs / 86400;
    // Approximate date from epoch days (good enough for meta timestamps).
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Gregorian calendar approximation from epoch (1970-01-01).
    let mut d = days as i64;
    let mut y = 1970i64;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    let mut remaining = d;
    for md in month_days.iter() {
        if remaining < *md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    (y as u64, month, remaining as u64 + 1)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /scim/v2/Users — list users.
pub async fn scim_list_users(
    headers: HeaderMap,
    Query(params): Query<ScimListQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check_scim_auth(&headers)?;

    let users = state.users.list().await?;
    let start_index = params.start_index.unwrap_or(1);
    let count = params.count.unwrap_or(usize::MAX);

    // Simple filter: userName eq "..."
    let filtered: Vec<User> = if let Some(filter) = &params.filter {
        if let Some(username) = filter
            .strip_prefix("userName eq \"")
            .and_then(|s| s.strip_suffix('"'))
        {
            users
                .into_iter()
                .filter(|u| u.username == username)
                .collect()
        } else {
            users
        }
    } else {
        users
    };

    let offset = (start_index.saturating_sub(1)) as usize;
    let page: Vec<ScimUser> = filtered
        .iter()
        .skip(offset)
        .take(count)
        .map(user_to_scim)
        .collect();
    let total = filtered.len();

    // Build response manually to use SCIM's "Resources" capitalisation.
    let resp = serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
        "totalResults": total,
        "startIndex": start_index,
        "itemsPerPage": page.len(),
        "Resources": page,
    });
    Ok(Json(resp))
}

/// POST /scim/v2/Users — create user.
pub async fn scim_create_user(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScimUserRequest>,
) -> Result<(StatusCode, Json<ScimUser>), ApiError> {
    check_scim_auth(&headers)?;

    let now = now_secs();
    let ext_id = req.external_id.clone().unwrap_or_default();

    // Idempotent: if external_id already exists, return existing user.
    if !ext_id.is_empty() {
        if let Some(existing) = state.users.find_by_external_id(&ext_id).await? {
            return Ok((StatusCode::OK, Json(user_to_scim(&existing))));
        }
    }

    let mut user = User::new(new_id(), ext_id, req.user_name.clone(), now);
    if let Some(dn) = req.display_name {
        user.display_name = dn;
    }
    if let Some(email) = req.emails.into_iter().find(|e| e.primary).map(|e| e.value) {
        user.email = Some(email);
    }

    state.users.create(&user).await?;
    Ok((StatusCode::CREATED, Json(user_to_scim(&user))))
}

/// GET /scim/v2/Users/{id} — get user.
pub async fn scim_get_user(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScimUser>, ApiError> {
    check_scim_auth(&headers)?;

    let user = state
        .users
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {id} not found")))?;

    Ok(Json(user_to_scim(&user)))
}

/// PUT /scim/v2/Users/{id} — replace user.
pub async fn scim_update_user(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScimUserRequest>,
) -> Result<Json<ScimUser>, ApiError> {
    check_scim_auth(&headers)?;

    let mut user = state
        .users
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {id} not found")))?;

    user.username = req.user_name.clone();
    if let Some(dn) = req.display_name {
        user.display_name = dn;
    }
    if let Some(email) = req.emails.into_iter().find(|e| e.primary).map(|e| e.value) {
        user.email = Some(email);
    }
    if let Some(ext_id) = req.external_id {
        user.external_id = ext_id;
    }
    user.updated_at = now_secs();

    state.users.update(&user).await?;
    Ok(Json(user_to_scim(&user)))
}

/// DELETE /scim/v2/Users/{id} — deactivate user (soft delete per SCIM spec).
pub async fn scim_delete_user(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    check_scim_auth(&headers)?;

    let user = state
        .users
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {id} not found")))?;

    // SCIM DELETE = deactivate, not hard delete. We delete from store here;
    // production deployments with SSO should soft-delete via `active=false`.
    state.users.delete(&user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /scim/v2/ServiceProviderConfig — SCIM capability document.
pub async fn scim_service_provider_config(
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    check_scim_auth(&headers)?;

    Ok(Json(serde_json::json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
        "documentationUri": "",
        "patch": { "supported": false },
        "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
        "filter": { "supported": true, "maxResults": 200 },
        "changePassword": { "supported": false },
        "sort": { "supported": false },
        "etag": { "supported": false },
        "authenticationSchemes": [{
            "type": "oauthbearertoken",
            "name": "OAuth Bearer Token",
            "description": "Authentication using the GYRE_SCIM_TOKEN bearer token"
        }],
        "meta": {
            "resourceType": "ServiceProviderConfig",
            "location": "/scim/v2/ServiceProviderConfig"
        }
    })))
}

/// GET /scim/v2/Schemas — resource type schemas.
pub async fn scim_schemas(headers: HeaderMap) -> Result<Json<serde_json::Value>, ApiError> {
    check_scim_auth(&headers)?;

    Ok(Json(serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
        "totalResults": 1,
        "Resources": [{
            "id": "urn:ietf:params:scim:schemas:core:2.0:User",
            "name": "User",
            "description": "User account",
            "attributes": [
                { "name": "userName", "type": "string", "required": true, "uniqueness": "server" },
                { "name": "displayName", "type": "string", "required": false },
                { "name": "emails", "type": "complex", "multiValued": true },
                { "name": "active", "type": "boolean" },
                { "name": "externalId", "type": "string" }
            ]
        }]
    })))
}

/// GET /scim/v2/ResourceTypes — supported resource types.
pub async fn scim_resource_types(headers: HeaderMap) -> Result<Json<serde_json::Value>, ApiError> {
    check_scim_auth(&headers)?;

    Ok(Json(serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
        "totalResults": 1,
        "Resources": [{
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
            "id": "User",
            "name": "User",
            "endpoint": "/Users",
            "schema": "urn:ietf:params:scim:schemas:core:2.0:User",
            "meta": {
                "resourceType": "ResourceType",
                "location": "/scim/v2/ResourceTypes/User"
            }
        }]
    })))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::api_router;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn make_state() -> Arc<AppState> {
        crate::mem::test_state()
    }

    fn app() -> axum::Router {
        api_router().with_state(make_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    fn scim_request(method: &str, path: &str, body: Option<serde_json::Value>) -> Request<Body> {
        let token = "test-scim-token";
        std::env::set_var("GYRE_SCIM_TOKEN", token);
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", format!("Bearer {token}"));
        if let Some(b) = body {
            builder = builder.header("Content-Type", "application/json");
            builder.body(Body::from(b.to_string())).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        }
    }

    #[tokio::test]
    async fn scim_list_users_empty() {
        let resp = app()
            .oneshot(scim_request("GET", "/scim/v2/Users", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["totalResults"], 0);
    }

    #[tokio::test]
    async fn scim_create_and_get_user() {
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({
            "userName": "alice",
            "displayName": "Alice Smith",
            "emails": [{ "value": "alice@example.com", "primary": true, "type": "work" }]
        });

        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["userName"], "alice");

        let get_resp = app
            .oneshot(scim_request("GET", &format!("/scim/v2/Users/{id}"), None))
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let got = body_json(get_resp).await;
        assert_eq!(got["id"], id);
        assert_eq!(got["userName"], "alice");
    }

    #[tokio::test]
    async fn scim_update_user() {
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({ "userName": "bob", "displayName": "Bob" });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update = serde_json::json!({ "userName": "bob-updated", "displayName": "Bob Updated" });
        let upd_resp = app
            .clone()
            .oneshot(scim_request(
                "PUT",
                &format!("/scim/v2/Users/{id}"),
                Some(update),
            ))
            .await
            .unwrap();
        assert_eq!(upd_resp.status(), StatusCode::OK);
        let updated = body_json(upd_resp).await;
        assert_eq!(updated["userName"], "bob-updated");
    }

    #[tokio::test]
    async fn scim_delete_user() {
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({ "userName": "carol" });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(scim_request(
                "DELETE",
                &format!("/scim/v2/Users/{id}"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        let get_resp = app
            .oneshot(scim_request("GET", &format!("/scim/v2/Users/{id}"), None))
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn scim_service_provider_config() {
        let resp = app()
            .oneshot(scim_request("GET", "/scim/v2/ServiceProviderConfig", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["filter"]["supported"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn scim_unauthorized_without_token() {
        std::env::set_var("GYRE_SCIM_TOKEN", "secret");
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/scim/v2/Users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
