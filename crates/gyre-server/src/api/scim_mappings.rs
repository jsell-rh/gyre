//! Workspace SCIM group-to-role mapping endpoints (task-266,
//! identity-security.md §Group-to-Workspace Mapping).
//!
//! `PUT /api/v1/workspaces/{id}/scim-mappings` stores the workspace's
//! whole mapping set `[{idp_group, workspace_role}]` durably (SQLite/PG
//! `scim_workspace_mappings`, migration 000068 — the set survives a
//! server restart). `GET` reads it back.
//!
//! Authorization: the route is registered in the ABAC
//! `RouteResourceMapping` (resource `scim_mapping`); the builtin
//! `builtin:scim-mapping-management` policy allows the `read`/`write`
//! actions for workspace Owner/Admin members, and the handler ADDITIONALLY
//! enforces the spec's "workspace admins configure" constraint through the
//! same `can_manage_membership` guard as membership management (global
//! Admin / TenantAdmin / workspace Owner/Admin only — a plain Developer
//! member is 403), plus tenant containment via `load_workspace_scoped`.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{ScimWorkspaceMapping, WorkspaceRole};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::error::ApiError;
use super::now_secs;
use crate::auth::AuthenticatedAgent;
use crate::AppState;

/// One mapping entry in the request/response body.
#[derive(Debug, Deserialize, Serialize)]
pub struct ScimMappingEntry {
    pub idp_group: String,
    pub workspace_role: String,
}

#[derive(Debug, Deserialize)]
pub struct PutScimMappingsRequest {
    #[serde(default)]
    pub mappings: Vec<ScimMappingEntry>,
}

#[derive(Debug, Serialize)]
pub struct ScimMappingsResponse {
    pub mappings: Vec<ScimMappingEntry>,
}

/// PUT /api/v1/workspaces/{id}/scim-mappings — replace the workspace's
/// mapping set (the request body is the authoritative set; an omitted
/// entry is removed).
pub async fn put_workspace_scim_mappings(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<PutScimMappingsRequest>,
) -> Result<(StatusCode, Json<ScimMappingsResponse>), ApiError> {
    let ws = load_workspace(&state, &auth, &workspace_id).await?;
    if !can_manage(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "SCIM mappings can only be configured by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }

    // Validate and normalize the whole set BEFORE persisting anything:
    // a single bad entry (unknown role, empty idp_group, duplicate group)
    // rejects the request, never a partially-applied mapping set.
    let now = now_secs();
    let mut seen = std::collections::HashSet::new();
    let mut mappings = Vec::with_capacity(req.mappings.len());
    for entry in &req.mappings {
        let group = entry.idp_group.trim();
        if group.is_empty() {
            return Err(ApiError::InvalidInput(
                "idp_group must not be empty".to_string(),
            ));
        }
        if !seen.insert(group.to_string()) {
            return Err(ApiError::Conflict(format!(
                "duplicate mapping for idp_group {group:?} — one role per IdP group"
            )));
        }
        let role = WorkspaceRole::parse_role(&entry.workspace_role).ok_or_else(|| {
            ApiError::InvalidInput(format!("Unknown workspace role: {}", entry.workspace_role))
        })?;
        mappings.push(ScimWorkspaceMapping::new(
            ws.id.clone(),
            group.to_string(),
            role,
            now,
        ));
    }

    state
        .scim_mappings
        .replace_for_workspace(&ws.id, &mappings)
        .await?;

    Ok((
        StatusCode::OK,
        Json(ScimMappingsResponse {
            mappings: mappings
                .iter()
                .map(|m| ScimMappingEntry {
                    idp_group: m.idp_group.clone(),
                    workspace_role: m.workspace_role.as_str().to_string(),
                })
                .collect(),
        }),
    ))
}

/// GET /api/v1/workspaces/{id}/scim-mappings — read the mapping set back.
pub async fn get_workspace_scim_mappings(
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScimMappingsResponse>, ApiError> {
    let ws = load_workspace(&state, &auth, &workspace_id).await?;
    if !can_manage(&state, &auth, &ws).await? {
        return Err(ApiError::Forbidden(
            "SCIM mappings can only be read by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }
    let mappings = state.scim_mappings.list_by_workspace(&ws.id).await?;
    Ok(Json(ScimMappingsResponse {
        mappings: mappings
            .iter()
            .map(|m| ScimMappingEntry {
                idp_group: m.idp_group.clone(),
                workspace_role: m.workspace_role.as_str().to_string(),
            })
            .collect(),
    }))
}

/// Load the workspace with tenant containment (same guard as the
/// membership-management surface in api/users.rs — 403 on cross-tenant,
/// 404 on unknown).
async fn load_workspace(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &str,
) -> Result<gyre_domain::Workspace, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if !auth.is_system_principal() && ws.tenant_id.to_string() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "cross-tenant workspace access denied".to_string(),
        ));
    }
    Ok(ws)
}

/// Whether the caller may configure this workspace's mappings: the system
/// principal, a global TenantAdmin (stored record authoritative — JWT
/// claims can lag a demotion), or a workspace Owner/Admin member. Same
/// guard as `can_manage_membership` in api/users.rs.
async fn can_manage(
    state: &AppState,
    auth: &AuthenticatedAgent,
    ws: &gyre_domain::Workspace,
) -> Result<bool, ApiError> {
    if auth.is_system_principal() || auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(true);
    }
    if let Some(uid) = &auth.user_id {
        if let Ok(Some(user)) = state.users.find_by_id(uid).await {
            if user.global_role == gyre_domain::GlobalRole::TenantAdmin {
                return Ok(true);
            }
        }
    }
    let Some(uid) = &auth.user_id else {
        return Ok(false);
    };
    Ok(state
        .workspace_memberships
        .find_by_user_and_workspace(uid, &ws.id)
        .await?
        .map(|m| m.role.can_manage_members())
        .unwrap_or(false))
}
