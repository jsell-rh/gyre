//! SCIM 2.0 (RFC 7643 / RFC 7644) provisioning endpoints.
//!
//! Base path: `/scim/v2`
//! Authentication: `GYRE_SCIM_TOKEN` env var (separate Bearer token from main API token).
//! The global `require_auth_middleware` (lib.rs) passes `/scim/v2/*` through
//! to these handlers; every handler enforces `check_scim_auth` itself
//! (fail-closed 403 when the token is unset, constant-time compare otherwise).
//!
//! Implements a subset sufficient for enterprise IdP (Okta, Entra ID, Keycloak) integration:
//! - Users CRUD (create, read, update, patch, delete/deactivate)
//! - Deactivation lifecycle (active:false / DELETE → revoke sessions,
//!   clear spawned_by attribution, ownerless-workspace cascade)
//! - Enterprise extension (RFC 7643 §4.3: organization, manager)
//! - ServiceProviderConfig, Schemas, ResourceTypes discovery

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
    // Constant-time compare (same primitive as the global token ladder in
    // lib.rs): a plain `!=` on bearer secrets leaks an early-exit timing
    // signal an attacker can use to recover the configured token prefix.
    if !crate::auth::tokens_equal(token, &expected) {
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

/// SCIM enterprise extension (RFC 7643 §4.3) — serialized under the
/// extension schema URN as a sibling of the core attributes.
/// `organization` carries the Gyre tenant id; `manager.value` the
/// manager's Gyre user id (identity-security.md §SCIM User Schema).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScimEnterprise {
    pub organization: Option<String>,
    pub manager: Option<ScimManagerRef>,
}

/// RFC 7643 manager reference: `value` holds the referenced user's id.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScimManagerRef {
    pub value: Option<String>,
}

/// SCIM User resource (RFC 7643 §4.1 + §4.3 enterprise extension).
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
    #[serde(
        rename = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
        default
    )]
    pub enterprise: ScimEnterprise,
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
    #[serde(
        rename = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
        default
    )]
    pub enterprise: ScimEnterprise,
}

/// RFC 7644 §3.5.2 PATCH request body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScimPatchRequest {
    #[serde(default)]
    operations: Vec<ScimPatchOp>,
}

/// One PATCH operation. `op` defaults to "replace" when absent (some IdPs
/// omit it); unknown ops are rejected by the applier.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScimPatchOp {
    op: Option<String>,
    path: Option<String>,
    value: Option<serde_json::Value>,
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
        schemas: vec![
            "urn:ietf:params:scim:schemas:core:2.0:User".to_string(),
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User".to_string(),
        ],
        id: user.id.to_string(),
        external_id: if user.external_id.is_empty() {
            None
        } else {
            Some(user.external_id.clone())
        },
        user_name: user.username.clone(),
        display_name: Some(user.display_name.clone()),
        // The serialized `active` flag mirrors the stored lifecycle state
        // (deactivated_at) — identity-security.md §Gyre-specific SCIM
        // behavior: SCIM `active: false` deactivates the user, so the
        // resource must report the deactivation back instead of a
        // hardcoded `true`.
        active: !user.is_deactivated(),
        emails,
        meta: ScimMeta {
            resource_type: "User".to_string(),
            location,
            created: Some(created),
            last_modified: Some(last_modified),
        },
        enterprise: ScimEnterprise {
            organization: user.tenant_id.as_ref().map(|t| t.to_string()),
            manager: Some(ScimManagerRef {
                value: user.manager_id.as_ref().map(|m| m.to_string()),
            }),
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

// ── Lifecycle helpers ─────────────────────────────────────────────────────────

/// Apply the SCIM enterprise extension (RFC 7643 §4.3) to a user record:
/// `organization` → tenant scope, `manager.value` → manager user id.
/// `None` fields leave the stored value untouched (PATCH partial update);
/// an explicit JSON `null` clears it.
fn apply_enterprise(user: &mut User, enterprise: &ScimEnterprise) {
    if let Some(org) = &enterprise.organization {
        user.tenant_id = if org.is_empty() {
            None
        } else {
            Some(Id::new(org.clone()))
        };
    }
    if let Some(manager) = &enterprise.manager {
        user.manager_id = manager
            .value
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|v| Id::new(v.clone()));
    }
}

/// Deactivate a user via the SCIM lifecycle (identity-security.md
/// §Gyre-specific SCIM behavior: `active: false` / DELETE deprovision) and
/// cascade the deactivation so it is a lifecycle event, not a flag flip:
///
/// - the stored record is deactivated (`User::deactivate`, soft — the row
///   remains resolvable for audit and attribution, mirroring the admin
///   PUT /api/v1/users/:id/deactivate semantics);
/// - every active session for the user is revoked
///   (`sessions.revoke_all_for_user`) — a pre-deactivation session token
///   fails authentication on its next replay;
/// - active impersonation sessions the user participates in are ended
///   (same cascade as the admin deactivation path);
/// - the user is removed from agents' `spawned_by` tracking, so agent
///   escalations/notifications no longer attribute to (or route to) the
///   deactivated user. Cleared for every attributed agent, including
///   non-active ones: dead agents can auto-restart
///   (restart_on_failure / stale-agent detector), and a resurrected agent
///   must not resume attributing to a departed user;
/// - workspaces the user owned are re-tracked for the ownerless flow
///   (`workspace_ownership::on_user_removed`), same as the admin path.
async fn deactivate_via_scim(state: &Arc<AppState>, user: &mut User) -> Result<(), ApiError> {
    let already = user.is_deactivated();
    let now = now_secs();
    user.deactivate(now);
    state.users.update(user).await?;

    // Audit trail: the provisioning token is a server-to-server credential
    // with no agent/user identity, so the event records the SCIM source in
    // its detail instead of an actor id (same event type as the admin
    // deactivation path — one trail for one lifecycle event).
    use gyre_domain::audit::{AuditEvent, AuditEventType, AuditOutcome};
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::Custom("user_deactivated".to_string()),
        None,
        Some(user.id.clone()),
        None,
        None,
        None,
        "user".to_string(),
        Some(user.id.to_string()),
        AuditOutcome::Success,
        serde_json::json!({
            "user_id": user.id.to_string(),
            "username": user.username,
            "tenant_id": user.tenant_id.as_ref().map(|t| t.to_string()),
            "source": "scim",
            "already_deactivated": already,
        }),
        None,
        None,
        now,
    );
    if let Err(e) = state.audit.record(&event).await {
        tracing::warn!(user_id = %user.id, "user_deactivated audit event recording failed: {e}");
    }

    // Session revocation: sign the user out everywhere.
    state.sessions.revoke_all_for_user(&user.id).await?;

    // Impersonation cascade (same as the admin deactivation path).
    crate::impersonation::end_active_sessions_for_user(state, user.id.as_str(), "user_deactivated")
        .await;

    // spawned_by de-attribution: no agent continues attributing to the
    // deactivated user.
    let user_id_str = user.id.as_str().to_string();
    for mut agent in state.agents.list().await? {
        if agent.spawned_by.as_deref() == Some(user_id_str.as_str()) {
            agent.spawned_by = None;
            state.agents.update(&agent).await?;
        }
    }

    // Ownerless cascade: re-track every workspace the user belonged to.
    crate::workspace_ownership::on_user_removed(state, &user.id).await;

    Ok(())
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
///
/// The username handle is derived from the SCIM `userName` exactly as the
/// SSO first-login path derives it from `preferred_username`
/// (auth.rs `find_or_create_user`): sanitized to the URL-safe handle
/// contract (user-management.md §Username vs Display Name), falling back
/// to the external id when sanitization yields nothing. A changed
/// `userName` on a later PUT is ignored — the handle is immutable after
/// creation, and since create-time sanitization may legitimately differ
/// from what the IdP echoes back, ignoring (not 409) keeps IdP sync loops
/// working.
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

    // URL-safe handle derivation, mirroring the SSO path's fallback chain
    // (auth.rs find_or_create_user): sanitize the IdP-provided name, then
    // sanitize the external id, and reject when neither yields a usable
    // handle — never persist a non-URL-safe or empty username through the
    // provisioning path. The external id is an opaque IdP identifier (RFC
    // 7644 SCIM externalId / RFC 7519 StringOrURI; Auth0 externalIds
    // contain '|'), not a URL-safe string, so the raw value must not be
    // persisted as the handle even when it is unique per caller.
    let username = match User::sanitize_username(&req.user_name) {
        Some(handle) => handle,
        None => User::sanitize_username(&ext_id).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "userName {:?} has no URL-safe characters and no URL-safe externalId to fall back to",
                req.user_name
            ))
        })?,
    };
    // Username is unique (spec §Username vs Display Name). The adapter
    // also enforces this, but checking here yields a precise 409 instead
    // of a raw storage error surfacing as 500 (same pattern as
    // api::users::create_user).
    if let Some(existing) = state.users.find_by_username(&username).await? {
        return Err(ApiError::Conflict(format!(
            "username {username} already taken (user {})",
            existing.id
        )));
    }

    let display_name = req.display_name.unwrap_or_else(|| req.user_name.clone());
    // Task-129 review round 3 (privilege escalation): display_name becomes
    // the user's ABAC subject.id under API-key/JWT auth, and
    // `system-full-access` (priority 1000) matches the reserved system
    // identity — it must not be provisionable.
    if display_name == gyre_domain::SYSTEM_ABAC_IDENTITY {
        return Err(ApiError::InvalidInput(format!(
            "displayName '{}' is reserved for the system principal",
            gyre_domain::SYSTEM_ABAC_IDENTITY
        )));
    }
    let mut user = User::new_sso(new_id(), ext_id, username, display_name, now);
    if let Some(email) = req.emails.into_iter().find(|e| e.primary).map(|e| e.value) {
        user.email = Some(email);
    }
    // Enterprise extension (RFC 7643 §4.3): organization binds the tenant
    // scope, manager.value records the manager's user id.
    apply_enterprise(&mut user, &req.enterprise);

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
///
/// `userName` and `externalId` are the immutable identity handle and the
/// immutable IdP join key (user-management.md §Username vs Display Name,
/// §User Entity): a changed value in the request is ignored rather than
/// rejected, because IdPs routinely re-send their original (pre-sanitized)
/// `userName` on every sync PUT and a 409 would break the sync loop. All
/// mutable profile fields (displayName, emails) are replaced per SCIM's
/// PUT semantics.
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

    // username and external_id stay untouched: immutable after creation.
    if let Some(dn) = req.display_name {
        // Task-129 review round 3: the reserved system ABAC identity must
        // not be assignable as a display_name (it is matched by
        // `system-full-access` at priority 1000).
        if dn == gyre_domain::SYSTEM_ABAC_IDENTITY {
            return Err(ApiError::InvalidInput(format!(
                "displayName '{}' is reserved for the system principal",
                gyre_domain::SYSTEM_ABAC_IDENTITY
            )));
        }
        user.display_name = dn;
    }
    if let Some(email) = req.emails.into_iter().find(|e| e.primary).map(|e| e.value) {
        user.email = Some(email);
    }
    // Enterprise extension (RFC 7643 §4.3): accepted and persisted on PUT.
    apply_enterprise(&mut user, &req.enterprise);

    // SCIM `active: false` on PUT is the IdP deactivation push
    // (identity-security.md §Gyre-specific SCIM behavior) — the same
    // lifecycle event PATCH and DELETE drive. An omitted `active` (or
    // `true`) never deactivates: deactivation is terminal and destructive
    // (sessions revoked, attribution cleared), so it must be an explicit
    // statement, not a PUT side effect of an attribute the IdP left out.
    // Re-activating an already-deactivated account is not supported
    // (deactivation is terminal, user-management.md) — the request is
    // accepted and the stored (inactive) state is reported back.
    if req.active == Some(false) && !user.is_deactivated() {
        deactivate_via_scim(&state, &mut user).await?;
    } else {
        user.updated_at = now_secs();
        state.users.update(&user).await?;
    }
    Ok(Json(user_to_scim(&user)))
}

/// PATCH /scim/v2/Users/{id} — partial update (RFC 7644 §3.5.2).
///
/// Accepts the standard PatchOp body (`Operations` array with
/// `op`/`path`/`value`) and, for IdPs that send a bare resource fragment,
/// a simplified direct body (`{"active": false}`) treated as one
/// path-less `replace`. Supported targets: `active` (the deactivation
/// path), `displayName`, `emails`, and the enterprise extension
/// attributes (`organization`, `manager` / `manager.value`). `userName`
/// and `externalId` are immutable after creation — attempts to patch them
/// are ignored (not 409), keeping IdP sync loops working, exactly as PUT.
///
/// ABAC-exempt route: authorization is the SCIM bearer token itself
/// (`check_scim_auth`, fail-closed when GYRE_SCIM_TOKEN is unset).
pub async fn scim_patch_user(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<Json<ScimUser>, ApiError> {
    check_scim_auth(&headers)?;

    let mut user = state
        .users
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {id} not found")))?;

    // Parse leniently: a body without an `Operations` member is treated as
    // a single path-less replace of its present attributes (the simplified
    // form `{{"active": false}}` some provisioning integrations send).
    let raw: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::InvalidInput(format!("invalid JSON: {e}")))?;
    let ops: Vec<ScimPatchOp> = if raw.get("Operations").is_some() {
        serde_json::from_value::<ScimPatchRequest>(raw)
            .map_err(|e| ApiError::InvalidInput(format!("invalid PatchOp request: {e}")))?
            .operations
    } else {
        vec![ScimPatchOp {
            op: Some("replace".to_string()),
            path: None,
            value: Some(raw),
        }]
    };

    let mut deactivate = false;
    for op in ops {
        let kind = op.op.as_deref().unwrap_or("replace").to_ascii_lowercase();
        let kind = kind.as_str();
        if !matches!(kind, "add" | "replace" | "remove") {
            return Err(ApiError::InvalidInput(format!(
                "unsupported PATCH op: {kind}"
            )));
        }
        // Normalize the target: path-less operations carry the attributes
        // in `value`; a path may be a core attribute, an enterprise
        // extension attribute (`<urn>:manager`), or a sub-attribute
        // (`<urn>:manager.value`). A trailing filter
        // (`emails[value eq "..."]`) is not supported — the whole
        // attribute is replaced.
        let path = op.path.as_deref().map(|p| {
            p.split('[')
                .next()
                .unwrap_or(p)
                .trim_end_matches("]")
                .to_string()
        });
        let value = op.value.unwrap_or(serde_json::Value::Null);

        match path.as_deref() {
            // Immutable identity attributes: ignored on any op (same
            // rationale as PUT — a 409 would break IdP sync loops).
            Some("userName") | Some("externalId") | Some("name") => {}
            Some("active") => {
                let want = value.as_bool().ok_or_else(|| {
                    ApiError::InvalidInput("active requires a boolean value".to_string())
                })?;
                if want {
                    // Reactivation is not supported (deactivation is
                    // terminal): report the stored state back honestly.
                } else if !user.is_deactivated() {
                    deactivate = true;
                }
            }
            Some("displayName") => {
                if let Some(dn) = value.as_str() {
                    if dn == gyre_domain::SYSTEM_ABAC_IDENTITY {
                        return Err(ApiError::InvalidInput(format!(
                            "displayName '{}' is reserved for the system principal",
                            gyre_domain::SYSTEM_ABAC_IDENTITY
                        )));
                    }
                    user.display_name = dn.to_string();
                }
            }
            Some("emails") => {
                if let Some(email) = patch_emails(&value) {
                    user.email = Some(email);
                }
            }
            Some("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User")
            | Some("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager")
            | Some("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager.value")
            | Some("urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:organization") => {
                patch_enterprise(&mut user, kind, path.as_deref().unwrap_or(""), &value)?;
            }
            None => {
                // Path-less replace: apply every recognized attribute in
                // the value object.
                let obj = value.as_object().ok_or_else(|| {
                    ApiError::InvalidInput(
                        "path-less PATCH operation requires an object value".to_string(),
                    )
                })?;
                for (key, val) in obj {
                    match key.as_str() {
                        "userName" | "externalId" | "schemas" | "id" | "name" => {}
                        "active" => {
                            if val == &serde_json::Value::Bool(false) && !user.is_deactivated() {
                                deactivate = true;
                            }
                        }
                        "displayName" => {
                            if let Some(dn) = val.as_str() {
                                if dn == gyre_domain::SYSTEM_ABAC_IDENTITY {
                                    return Err(ApiError::InvalidInput(format!(
                                        "displayName '{}' is reserved for the system principal",
                                        gyre_domain::SYSTEM_ABAC_IDENTITY
                                    )));
                                }
                                user.display_name = dn.to_string();
                            }
                        }
                        "emails" => {
                            if let Some(email) = patch_emails(val) {
                                user.email = Some(email);
                            }
                        }
                        "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User" => {
                            patch_enterprise(
                                &mut user,
                                kind,
                                "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
                                val,
                            )?;
                        }
                        _ => {
                            // Unknown attributes are ignored per RFC 7644
                            // §3.5.2 leniency for unrecognized extensions.
                        }
                    }
                }
            }
            Some(other) => {
                // Unknown path: ignore (RFC 7644 leniency) rather than
                // fail the whole PATCH — IdPs routinely send attributes
                // outside the implemented subset.
                let _ = other;
            }
        }
    }

    if deactivate {
        deactivate_via_scim(&state, &mut user).await?;
    } else {
        user.updated_at = now_secs();
        state.users.update(&user).await?;
    }
    Ok(Json(user_to_scim(&user)))
}

/// Extract the primary email from a PATCH `emails` value (single object or
/// array); `None` leaves the stored email untouched.
fn patch_emails(value: &serde_json::Value) -> Option<String> {
    let items: Vec<&serde_json::Value> = match value {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(_) => vec![value],
        _ => return None,
    };
    let primary = items
        .iter()
        .find(|e| e.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .copied()
        .or_else(|| items.first().copied());
    primary
        .and_then(|e| e.get("value").and_then(|v| v.as_str()))
        .map(str::to_string)
}

/// Apply an enterprise-extension PATCH target to the user record.
fn patch_enterprise(
    user: &mut User,
    kind: &str,
    path: &str,
    value: &serde_json::Value,
) -> Result<(), ApiError> {
    const URN: &str = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
    if path == URN {
        // Whole-extension replace: value is the extension object.
        let ext: ScimEnterprise = serde_json::from_value(value.clone()).map_err(|e| {
            ApiError::InvalidInput(format!("invalid enterprise extension value: {e}"))
        })?;
        if kind == "remove" {
            user.tenant_id = None;
            user.manager_id = None;
        } else {
            apply_enterprise(user, &ext);
        }
        return Ok(());
    }
    let attr = path.strip_prefix(URN).and_then(|r| r.strip_prefix(':'));
    match (kind, attr) {
        ("remove", Some("organization")) => user.tenant_id = None,
        ("remove", Some("manager")) | ("remove", Some("manager.value")) => user.manager_id = None,
        (_, Some("organization")) => {
            if let Some(org) = value.as_str() {
                user.tenant_id = if org.is_empty() {
                    None
                } else {
                    Some(Id::new(org.to_string()))
                };
            }
        }
        (_, Some("manager")) | (_, Some("manager.value")) => {
            let mid = match &value {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(o) => {
                    o.get("value").and_then(|v| v.as_str()).map(str::to_string)
                }
                _ => None,
            };
            user.manager_id = mid.filter(|v| !v.is_empty()).map(Id::new);
        }
        _ => {}
    }
    Ok(())
}

/// DELETE /scim/v2/Users/{id} — deprovision: mark the user inactive
/// (identity-security.md: "Deprovision (marks user inactive, does not
/// delete)"). Soft deactivation with the full lifecycle cascade (sessions
/// revoked, spawned_by attribution cleared, ownerless-workspace flow) —
/// the row remains resolvable so audit history and lingering memberships
/// survive, mirroring the admin deactivation semantics.
pub async fn scim_delete_user(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    check_scim_auth(&headers)?;

    let mut user = state
        .users
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {id} not found")))?;

    // SCIM DELETE = deprovision = deactivate, never a hard store delete:
    // the user row must remain resolvable (GET /scim/v2/Users/{id} still
    // returns it with active:false) so the audit trail, attribution, and
    // lingering workspace memberships survive the departure.
    deactivate_via_scim(&state, &mut user).await?;

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
        "patch": { "supported": true },
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
        "totalResults": 2,
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
        }, {
            "id": "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
            "name": "EnterpriseUser",
            "description": "Enterprise user extension (RFC 7643 §4.3)",
            "attributes": [
                { "name": "organization", "type": "string", "required": false,
                  "description": "Gyre tenant id" },
                { "name": "manager", "type": "complex", "required": false,
                  "subAttributes": [ { "name": "value", "type": "string" } ] }
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
            "schemaExtensions": [{
                "schema": "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
                "required": false
            }],
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
    async fn scim_update_user() {
        // PUT replaces mutable profile fields (displayName) but ignores
        // userName/externalId: the username handle is immutable after
        // creation (user-management.md §Username vs Display Name), and
        // IdPs re-send their original userName on every sync PUT.
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({ "userName": "Bob", "displayName": "Bob" });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(
            created["userName"], "bob",
            "userName is sanitized on create"
        );

        let update = serde_json::json!({
            "userName": "bob-renamed",
            "displayName": "Bob Updated",
            "externalId": "evil-swapped-identity"
        });
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
        assert_eq!(
            updated["userName"], "bob",
            "username is immutable after creation"
        );
        assert_eq!(updated["displayName"], "Bob Updated");

        // externalId swap attempt did not take either (immutable join key).
        let get_resp = app
            .clone()
            .oneshot(scim_request("GET", &format!("/scim/v2/Users/{id}"), None))
            .await
            .unwrap();
        let got = body_json(get_resp).await;
        assert_eq!(got["externalId"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn scim_rejects_reserved_system_identity() {
        // Task-129 review round 3 (privilege escalation): SCIM displayName
        // becomes the user's ABAC subject.id under API-key/JWT auth, and
        // `system-full-access` (priority 1000) matches the reserved system
        // identity — neither create nor update may assign it.
        let app = api_router().with_state(make_state());

        // Create with the reserved displayName (and, when displayName is
        // absent, the raw userName fallback) is rejected.
        let payload = serde_json::json!({
            "userName": "spoof",
            "displayName": gyre_domain::SYSTEM_ABAC_IDENTITY
        });
        let resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "SCIM create must reject the reserved system ABAC identity as displayName"
        );

        let payload = serde_json::json!({ "userName": gyre_domain::SYSTEM_ABAC_IDENTITY });
        let resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "SCIM create must reject the reserved identity when it arrives as userName \
             (displayName defaults to it)"
        );

        // Update of an existing user to the reserved displayName is
        // rejected and does not apply.
        let payload = serde_json::json!({ "userName": "Bob", "displayName": "Bob" });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update = serde_json::json!({
            "userName": "bob",
            "displayName": gyre_domain::SYSTEM_ABAC_IDENTITY
        });
        let resp = app
            .clone()
            .oneshot(scim_request(
                "PUT",
                &format!("/scim/v2/Users/{id}"),
                Some(update),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "SCIM update must reject the reserved system ABAC identity as displayName"
        );

        let get_resp = app
            .oneshot(scim_request("GET", &format!("/scim/v2/Users/{id}"), None))
            .await
            .unwrap();
        let got = body_json(get_resp).await;
        assert_eq!(
            got["displayName"], "Bob",
            "rejected reserved-identity update must not apply"
        );
    }

    #[tokio::test]
    async fn scim_create_sanitizes_user_name() {
        // The provisioning path must derive the same URL-safe handle the
        // SSO path derives; "Bad Handle!" has URL-safe characters so it
        // sanitizes to "bad-handle" (display name keeps the raw value).
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({
            "userName": "Bad Handle!",
            "displayName": "Bad Handle!"
        });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["userName"], "bad-handle");
        assert_eq!(created["displayName"], "Bad Handle!");
    }

    #[tokio::test]
    async fn scim_create_sanitizes_falls_back_to_external_id() {
        // A userName with nothing URL-safe falls back to the externalId;
        // with no externalId either, the request is rejected with 400 —
        // never an empty or non-URL-safe persisted username.
        let app = api_router().with_state(make_state());
        let payload = serde_json::json!({
            "userName": "!!!",
            "externalId": "ext-fall-back"
        });
        let create_resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(payload)))
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["userName"], "ext-fall-back");

        // The externalId fallback is itself sanitized: externalIds are
        // opaque IdP identifiers (Auth0 externalIds contain '|'), so the
        // raw value must not be persisted as the handle even though it is
        // unique per caller — mirrors find_or_create_user's subject-
        // fallback chain (auth.rs).
        let opaque = serde_json::json!({
            "userName": "!!!",
            "externalId": "auth0|67890"
        });
        let resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(opaque)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        assert_eq!(created["userName"], "auth067890");
        assert!(
            gyre_domain::User::validate_username(created["userName"].as_str().unwrap()).is_ok(),
            "fallback username must satisfy the URL-safe handle contract"
        );

        // Neither userName nor externalId yields anything URL-safe: reject.
        let neither = serde_json::json!({ "userName": "!!!", "externalId": "!?!" });
        let resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(neither)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let no_fallback = serde_json::json!({ "userName": "!!!" });
        let resp = app
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(no_fallback)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn scim_create_conflict_on_duplicate_username() {
        // Two distinct external ids colliding on the sanitized handle:
        // the second create must be a 409, not a 500 from the storage
        // layer's unique-index violation.
        let app = api_router().with_state(make_state());
        let first = serde_json::json!({ "userName": "carol", "externalId": "ext-c1" });
        let resp = app
            .clone()
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(first)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let second = serde_json::json!({ "userName": "Carol!", "externalId": "ext-c2" });
        let resp = app
            .oneshot(scim_request("POST", "/scim/v2/Users", Some(second)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
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
    async fn scim_delete_user() {
        // SCIM DELETE deprovisions (marks the user inactive) — never a hard
        // store delete: the row remains resolvable and reports active:false
        // (identity-security.md §SCIM Endpoints: "Deprovision (marks user
        // inactive, does not delete)").
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
        assert_eq!(
            get_resp.status(),
            StatusCode::OK,
            "deprovisioned user row must remain resolvable (soft deactivation)"
        );
        let got = body_json(get_resp).await;
        assert_eq!(got["active"], serde_json::Value::Bool(false));
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
        assert!(
            json["patch"]["supported"].as_bool().unwrap_or_default(),
            "PATCH /scim/v2/Users/{{id}} is implemented — ServiceProviderConfig must advertise it"
        );
    }

    #[tokio::test]
    async fn scim_unauthorized_without_token() {
        // Configure the SAME token value every other SCIM test writes:
        // GYRE_SCIM_TOKEN is process-global and the lib tests run in parallel,
        // so pointing it at a divergent value here races the in-flight
        // handlers of the other SCIM tests (their bearer token no longer
        // matches the configured one, their creates turn into 403s, and
        // scim_delete_user panics on the missing "id"). The rejection under
        // test comes from the missing Authorization header, which fails
        // against any configured token value.
        std::env::set_var("GYRE_SCIM_TOKEN", "test-scim-token");
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
