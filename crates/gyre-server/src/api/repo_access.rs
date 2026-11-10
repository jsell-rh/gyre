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

    state
        .repo_access_overrides
        .create(&over)
        .await
        .map_err(|e| {
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
    let items: Vec<RepoAccessOverrideResponse> = overrides.into_iter().map(Into::into).collect();
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
    let removed = state.repo_access_overrides.delete(&repo.id, &uid).await?;
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
            ApiError::NotFound(format!(
                "user {uid} is not a member of the repo's workspace"
            ))
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

#[cfg(test)]
mod tests {
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::{Repository, User, Workspace, WorkspaceMembership, WorkspaceRole};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Seed a workspace + repo in tenant t-1 plus a member user with the
    /// given workspace role. Returns (app router, state, repo_id, user_id).
    /// The member gets an API key (`dev-repo-key`) authenticating as a
    /// Developer-role user so ABAC read/write actions pass and the
    /// membership/override logic under test is what answers.
    async fn seed(
        role: WorkspaceRole,
    ) -> (Router, std::sync::Arc<crate::AppState>, String, String) {
        let state = crate::mem::test_state();
        let now = 1_700_000_000u64;
        let tenant = gyre_domain::Tenant::new(Id::new("t-1"), "Acme", "acme", 0);
        state.tenants.create(&tenant).await.unwrap();

        let ws_id = uuid::Uuid::new_v4().to_string();
        let ws = Workspace::new(
            Id::new(ws_id.clone()),
            Id::new("t-1"),
            format!("repo-access-ws-{ws_id}"),
            format!("repo-access-ws-{ws_id}"),
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let repo_id = uuid::Uuid::new_v4().to_string();
        let repo = Repository::new(
            Id::new(repo_id.clone()),
            Id::new(ws_id.clone()),
            format!("repo-{repo_id}"),
            format!("/tmp/gyre-test/{repo_id}"),
            now,
        );
        state.repos.create(&repo).await.unwrap();

        let user_id = uuid::Uuid::new_v4().to_string();
        let mut user = User::new(
            Id::new(user_id.clone()),
            format!("member-ext-{user_id}"),
            format!("member-{user_id}"),
            now,
        );
        user.tenant_id = Some(Id::new("t-1"));
        user.global_role = gyre_domain::GlobalRole::Member;
        user.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&user).await.unwrap();

        let membership = WorkspaceMembership::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(user_id.clone()),
            Id::new(ws_id),
            role,
            Id::new("system"),
            now,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key("dev-repo-key"),
                &Id::new(user_id.clone()),
                "test",
            )
            .await
            .unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        (app, state, repo_id, user_id)
    }

    #[tokio::test]
    async fn override_restricts_effective_permissions() {
        // user-management.md §Repo Access: a per-repo override restricts a
        // Developer to Viewer on one repo — the effective permission set
        // collapses to read-only while the workspace role stays Developer.
        let (app, state, repo_id, user_id) = seed(WorkspaceRole::Developer).await;

        // Before the override: Developer permissions per the spec table.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/repos/{repo_id}/access/{user_id}"))
                    .header("Authorization", "Bearer dev-repo-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let access = body_json(resp).await;
        assert_eq!(access["workspace_role"], "Developer");
        assert_eq!(access["effective_role"], "Developer");
        assert!(access["permissions"].as_array().unwrap().len() > 1);

        // Restrict the Developer to Viewer on this repo.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/access-overrides/{user_id}"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"role": "Viewer"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        assert_eq!(created["restricted_role"], "Viewer");

        // The effective access view now shows the restricted role and a
        // read-only permission set.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/repos/{repo_id}/access/{user_id}"))
                    .header("Authorization", "Bearer dev-repo-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let access = body_json(resp).await;
        assert_eq!(
            access["workspace_role"], "Developer",
            "workspace role unchanged"
        );
        assert_eq!(access["override_role"], "Viewer");
        assert_eq!(access["effective_role"], "Viewer");
        assert_eq!(
            access["permissions"],
            serde_json::json!(["read"]),
            "a Developer restricted to Viewer must be read-only on this repo"
        );

        // Lifting the override restores the workspace role.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/access-overrides/{user_id}"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            state
                .repo_access_overrides
                .find_by_repo_and_user(&Id::new(repo_id), &Id::new(user_id))
                .await
                .unwrap()
                .is_none(),
            "the override must be gone after DELETE"
        );
    }

    #[tokio::test]
    async fn override_cannot_expand_beyond_workspace_role() {
        // user-management.md §Repo Access: overrides can restrict but never
        // expand — capping a Viewer at Developer must be rejected.
        let (app, _state, repo_id, user_id) = seed(WorkspaceRole::Viewer).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/access-overrides/{user_id}"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"role": "Developer"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "an override that would expand access must be rejected"
        );
    }

    #[tokio::test]
    async fn override_create_is_idempotent_rejected() {
        // The port contract: an existing (repo, user) override is a 409, not
        // a silent rewrite.
        let (app, _state, repo_id, user_id) = seed(WorkspaceRole::Developer).await;

        let req = || {
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v1/repos/{repo_id}/access-overrides/{user_id}"
                ))
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({"role": "Viewer"}).to_string(),
                ))
                .unwrap()
        };
        let resp = app.clone().oneshot(req()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app.oneshot(req()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "setting a second override for the same (repo, user) must be a 409"
        );
    }

    #[tokio::test]
    async fn override_requires_workspace_membership() {
        // An override caps a membership role — there is nothing to restrict
        // for a non-member.
        let (app, _state, repo_id, _user_id) = seed(WorkspaceRole::Developer).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/access-overrides/nobody-home"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"role": "Viewer"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "restricting a non-member must be a 404"
        );
    }
}
