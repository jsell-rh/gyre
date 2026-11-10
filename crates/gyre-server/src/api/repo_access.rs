//! Repo access endpoints (user-management.md §Repo Access).
//!
//! Repo access is inherited from workspace membership; per-repo overrides
//! can restrict (but not expand) access beyond the workspace role.
//!
//! PUT    /api/v1/repos/:id/access-overrides/:user_id   (restrict a member on one repo)
//! GET    /api/v1/repos/:id/access-overrides            (list overrides)
//! DELETE /api/v1/repos/:id/access-overrides/:user_id   (lift a restriction)
//! GET    /api/v1/repos/:id/access/:user_id             (effective access view)
//!
//! Override management is an Owner/Admin capability on the repo's workspace
//! (the workspace Admin role carries repo `admin` per the §Repo Access
//! table). Overrides may only restrict: an override whose role is more
//! privileged than the target's workspace role is rejected.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    effective_repo_permissions, effective_repo_role, RepoAccessOverride, WorkspaceRole,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Serialize)]
pub struct RepoAccessOverrideResponse {
    pub id: String,
    pub repo_id: String,
    pub user_id: String,
    pub restricted_role: String,
    pub created_by: String,
    pub created_at: u64,
}

impl From<RepoAccessOverride> for RepoAccessOverrideResponse {
    fn from(o: RepoAccessOverride) -> Self {
        Self {
            id: o.id.to_string(),
            repo_id: o.repo_id.to_string(),
            user_id: o.user_id.to_string(),
            restricted_role: o.restricted_role.as_str().to_string(),
            created_by: o.created_by.to_string(),
            created_at: o.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct SetOverrideRequest {
    pub role: String,
}

/// Load a repo and enforce the caller's tenant scope on it.
async fn load_repo_scoped(
    state: &AppState,
    auth: &AuthenticatedAgent,
    repo_id: &str,
) -> Result<gyre_domain::Repository, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    if let Ok(Some(ws)) = state.workspaces.find_by_id(&repo.workspace_id).await {
        if !auth.is_system_principal() && ws.tenant_id.to_string() != auth.tenant_id {
            return Err(ApiError::Forbidden(
                "cross-tenant repo access denied".to_string(),
            ));
        }
    }
    Ok(repo)
}

/// Whether the caller may manage repo access overrides: the system
/// principal, a global TenantAdmin, or an Owner/Admin member of the repo's
/// workspace (the §Repo Access table gives both roles repo `admin`).
async fn can_manage_repo_access(
    state: &AppState,
    auth: &AuthenticatedAgent,
    repo: &gyre_domain::Repository,
) -> Result<bool, ApiError> {
    if auth.is_system_principal() || auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Ok(true);
    }
    let Some(uid) = &auth.user_id else {
        return Ok(false);
    };
    if let Ok(Some(user)) = state.users.find_by_id(uid).await {
        if user.global_role == gyre_domain::GlobalRole::TenantAdmin {
            return Ok(true);
        }
    }
    Ok(state
        .workspace_memberships
        .find_by_user_and_workspace(uid, &repo.workspace_id)
        .await?
        .map(|m| m.role.can_manage_members())
        .unwrap_or(false))
}

/// PUT /api/v1/repos/:id/access-overrides/:user_id — restrict a member's
/// role on one repo.
pub async fn set_repo_access_override(
    auth: AuthenticatedAgent,
    Path((repo_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetOverrideRequest>,
) -> Result<(StatusCode, Json<RepoAccessOverrideResponse>), ApiError> {
    let restricted_role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;
    let repo = load_repo_scoped(&state, &auth, &repo_id).await?;
    if !can_manage_repo_access(&state, &auth, &repo).await? {
        return Err(ApiError::Forbidden(
            "repo access overrides can only be managed by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }

    let uid = Id::new(user_id);
    // The target must be a member of the repo's workspace — an override
    // caps a membership role, so there is nothing to restrict otherwise.
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &repo.workspace_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "user {uid} is not a member of the repo's workspace"
            ))
        })?;

    // Restrict-only invariant (user-management.md §Repo Access): the
    // override may not grant capabilities beyond the workspace role.
    let over = RepoAccessOverride::new(
        new_id(),
        repo.id.clone(),
        uid.clone(),
        restricted_role,
        Id::new(auth.agent_id.clone()),
        now_secs(),
    );
    over.validate(membership.role)
        .map_err(ApiError::InvalidInput)?;

    state.repo_access_overrides.create(&over).await.map_err(|e| {
        if e.to_string().contains("already exists") {
            ApiError::Conflict(format!(
                "an access override for user {uid} on repo {repo_id} already exists; delete it \
                 before setting a new one"
            ))
        } else {
            ApiError::Internal(e)
        }
    })?;

    Ok((
        StatusCode::CREATED,
        Json(RepoAccessOverrideResponse::from(over)),
    ))
}

/// GET /api/v1/repos/:id/access-overrides — list the repo's overrides.
pub async fn list_repo_access_overrides(
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = load_repo_scoped(&state, &auth, &repo_id).await?;
    if !can_manage_repo_access(&state, &auth, &repo).await? {
        return Err(ApiError::Forbidden(
            "repo access overrides can only be managed by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }
    let overrides = state.repo_access_overrides.list_by_repo(&repo.id).await?;
    let items: Vec<RepoAccessOverrideResponse> =
        overrides.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "overrides": items })))
}

/// DELETE /api/v1/repos/:id/access-overrides/:user_id — lift a restriction.
pub async fn delete_repo_access_override(
    auth: AuthenticatedAgent,
    Path((repo_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let repo = load_repo_scoped(&state, &auth, &repo_id).await?;
    if !can_manage_repo_access(&state, &auth, &repo).await? {
        return Err(ApiError::Forbidden(
            "repo access overrides can only be managed by a workspace Owner, Admin, or TenantAdmin"
                .to_string(),
        ));
    }
    let uid = Id::new(user_id);
    let removed = state
        .repo_access_overrides
        .delete(&repo.id, &uid)
        .await?;
    if !removed {
        return Err(ApiError::NotFound(format!(
            "no access override for user {uid} on repo {repo_id}"
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/repos/:id/access/:user_id — the user's effective access on
/// this repo: workspace role, per-repo override, effective role, and the
/// derived permission set (user-management.md §Repo Access table).
pub async fn get_repo_access(
    auth: AuthenticatedAgent,
    Path((repo_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = load_repo_scoped(&state, &auth, &repo_id).await?;
    // Any workspace member (or the caller themself) may read access info —
    // it is workspace data and the derived set is read-only knowledge.
    let uid = Id::new(user_id);
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &repo.workspace_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("user {uid} is not a member of the repo's workspace"))
        })?;

    let override_role = state
        .repo_access_overrides
        .find_by_repo_and_user(&repo.id, &uid)
        .await?
        .map(|o| o.restricted_role);
    let effective = effective_repo_role(membership.role, override_role);
    let permissions: Vec<&str> = effective_repo_permissions(membership.role, override_role)
        .iter()
        .map(|p| p.as_str())
        .collect();

    Ok(Json(serde_json::json!({
        "repo_id": repo.id.to_string(),
        "user_id": uid.to_string(),
        "workspace_role": membership.role.as_str(),
        "override_role": override_role.map(|r| r.as_str()),
        "effective_role": effective.as_str(),
        "permissions": permissions,
    })))
}
