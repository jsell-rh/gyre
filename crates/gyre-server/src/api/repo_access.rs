//! Per-repo access override endpoints (user-management.md §Repo Access).
//!
//! GET    /api/v1/repos/:id/access-overrides          (list restrictions)
//! PUT    /api/v1/repos/:id/access-overrides/:user_id (set restriction)
//! DELETE /api/v1/repos/:id/access-overrides/:user_id (remove restriction)
//!
//! Overrides can restrict but never expand: the member's effective role on
//! the repo is the more restrictive of their workspace role and the override
//! role. Setting an override above the member's workspace role is rejected
//! (a Viewer cannot be elevated to Developer on a specific repo without
//! changing their workspace role).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{RepoAccessOverride, WorkspaceRole};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Serialize)]
pub struct AccessOverrideResponse {
    pub id: String,
    pub repo_id: String,
    pub user_id: String,
    pub role: String,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<RepoAccessOverride> for AccessOverrideResponse {
    fn from(o: RepoAccessOverride) -> Self {
        Self {
            id: o.id.to_string(),
            repo_id: o.repo_id.to_string(),
            user_id: o.user_id.to_string(),
            role: o.role.as_str().to_string(),
            created_by: o.created_by.to_string(),
            created_at: o.created_at,
            updated_at: o.updated_at,
        }
    }
}

async fn load_repo(state: &AppState, repo_id: &str) -> Result<gyre_domain::Repository, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))
}

/// GET /api/v1/repos/:id/access-overrides — list the per-repo restrictions.
/// Managing repo configuration is an Admin-level permission
/// (user-management.md §Repo Access).
pub async fn list_access_overrides(
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = load_repo(&state, &repo_id).await?;
    crate::workspace_governance::check_repo_access_scope(
        &state,
        &repo,
        &auth,
        gyre_domain::RepoPermission::Admin,
    )
    .await?;
    let overrides = state.repo_access_overrides.list_by_repo(&repo.id).await?;
    let items: Vec<AccessOverrideResponse> = overrides.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "overrides": items })))
}

#[derive(Deserialize)]
pub struct SetAccessOverrideRequest {
    /// The role the member is restricted to on this repo. Must not exceed
    /// the member's workspace role (restrict-only).
    pub role: String,
}

/// PUT /api/v1/repos/:id/access-overrides/:user_id — set (or update) the
/// per-repo restriction for a member.
pub async fn set_access_override(
    auth: AuthenticatedAgent,
    Path((repo_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetAccessOverrideRequest>,
) -> Result<(StatusCode, Json<AccessOverrideResponse>), ApiError> {
    let repo = load_repo(&state, &repo_id).await?;
    // Only workspace Owner/Admin (or tenant administration) may configure
    // repo access.
    crate::workspace_governance::check_repo_access_scope(
        &state,
        &repo,
        &auth,
        gyre_domain::RepoPermission::Admin,
    )
    .await?;

    let role = WorkspaceRole::parse_role(&req.role)
        .ok_or_else(|| ApiError::InvalidInput(format!("Unknown role: {}", req.role)))?;
    let uid = Id::new(user_id);

    // The target must be an active member of the repo's workspace — an
    // override on a non-member is meaningless (there is no access to
    // restrict).
    let membership = state
        .workspace_memberships
        .find_by_user_and_workspace(&uid, &repo.workspace_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "user {uid} is not a member of the repo's workspace"
            ))
        })?;
    if !membership.is_active() {
        return Err(ApiError::InvalidInput(format!(
            "user {uid} has a pending invitation; accept it before restricting repo access"
        )));
    }

    // Restrict-only: reject overrides that would expand access beyond the
    // workspace role (a Viewer cannot be elevated to Developer on one repo).
    if RepoAccessOverride::validate_restriction(role, membership.role).is_err() {
        return Err(ApiError::InvalidInput(format!(
            "override role {} exceeds member's workspace role {} — per-repo overrides can \
             restrict but not expand access",
            role.as_str(),
            membership.role.as_str()
        )));
    }

    let caller_id = auth
        .user_id
        .clone()
        .unwrap_or_else(|| Id::new(auth.agent_id.clone()));

    // Upsert on the (repo, user) pair: one override per member per repo.
    if let Some(existing) = state
        .repo_access_overrides
        .find_by_repo_and_user(&repo.id, &uid)
        .await?
    {
        state
            .repo_access_overrides
            .update_role(&existing.id, role)
            .await?;
        let updated = state
            .repo_access_overrides
            .find_by_id(&existing.id)
            .await?
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("override vanished mid-update")))?;
        return Ok((StatusCode::OK, Json(AccessOverrideResponse::from(updated))));
    }

    let override_ =
        RepoAccessOverride::new(new_id(), repo.id.clone(), uid, role, caller_id, now_secs());
    state.repo_access_overrides.create(&override_).await?;
    Ok((
        StatusCode::CREATED,
        Json(AccessOverrideResponse::from(override_)),
    ))
}

/// DELETE /api/v1/repos/:id/access-overrides/:user_id — remove the per-repo
/// restriction (the member falls back to their full workspace role).
pub async fn delete_access_override(
    auth: AuthenticatedAgent,
    Path((repo_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, ApiError> {
    let repo = load_repo(&state, &repo_id).await?;
    crate::workspace_governance::check_repo_access_scope(
        &state,
        &repo,
        &auth,
        gyre_domain::RepoPermission::Admin,
    )
    .await?;

    let uid = Id::new(user_id);
    let existing = state
        .repo_access_overrides
        .find_by_repo_and_user(&repo.id, &uid)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "no access override for user {uid} on repo {repo_id}"
            ))
        })?;
    state.repo_access_overrides.delete(&existing.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::Request;
    use tower::ServiceExt;

    use crate::mem::test_state;
    use gyre_domain::{Repository, Workspace};

    async fn seed_workspace_with_members() -> (Arc<crate::AppState>, String, String, String) {
        let state = test_state();
        let ws = Workspace::new(
            Id::new("ws-ov"),
            Id::new("default"),
            "Override WS",
            "override-ws",
            1000,
        );
        state.workspaces.create(&ws).await.unwrap();
        let repo = Repository::new(
            Id::new("repo-ov"),
            ws.id.clone(),
            "app",
            "/tmp/app.git",
            1000,
        );
        state.repos.create(&repo).await.unwrap();

        // owner (accepted) and developer (accepted).
        for (uid, role) in [
            ("owner-1", WorkspaceRole::Owner),
            ("dev-1", WorkspaceRole::Developer),
        ] {
            let mut m = gyre_domain::WorkspaceMembership::new(
                Id::new(format!("m-{uid}")),
                Id::new(uid),
                ws.id.clone(),
                role,
                Id::new("owner-1"),
                1000,
            );
            m.accept(1500);
            state.workspace_memberships.create(&m).await.unwrap();
        }
        (state, repo.id.to_string(), "owner-1".into(), "dev-1".into())
    }

    async fn put_override(
        app: axum::Router,
        repo: &str,
        user: &str,
        role: &str,
    ) -> http::Response<axum::body::Body> {
        app.oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/repos/{repo}/access-overrides/{user}"))
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(r#"{{"role":"{role}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn override_restricts_developer_to_viewer() {
        let (state, repo, _owner, dev) = seed_workspace_with_members().await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = put_override(app, &repo, &dev, "Viewer").await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        // The developer is now restricted to Viewer on this repo: a Write
        // operation must be denied while Read is still allowed.
        let check = |perm| {
            let state = state.clone();
            let repo_id = repo.clone();
            async move {
                let r = state
                    .repos
                    .find_by_id(&Id::new(&repo_id))
                    .await
                    .unwrap()
                    .unwrap();
                let auth = crate::auth::AuthenticatedAgent {
                    agent_id: "dev-1".to_string(),
                    user_id: Some(Id::new("dev-1")),
                    roles: vec![gyre_domain::UserRole::Developer],
                    tenant_id: "default".to_string(),
                    jwt_claims: None,
                    deprecated_token_auth: false,
                };
                crate::workspace_governance::check_repo_access_scope(&state, &r, &auth, perm).await
            }
        };
        assert!(
            check(gyre_domain::RepoPermission::Read).await.is_ok(),
            "Viewer keeps read access"
        );
        assert!(
            check(gyre_domain::RepoPermission::Write).await.is_err(),
            "override must restrict Developer write access"
        );
    }

    #[tokio::test]
    async fn override_cannot_expand_viewer_to_developer() {
        // Restrict the Owner... no — expansion test: give dev-1 an override
        // ABOVE their workspace role; must be rejected.
        let (state, repo, _owner, dev) = seed_workspace_with_members().await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = put_override(app, &repo, &dev, "Admin").await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "override above workspace role must be rejected"
        );

        // And nothing was stored.
        assert!(state
            .repo_access_overrides
            .find_by_repo_and_user(&Id::new(&repo), &Id::new(&dev))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn put_is_upsert_and_delete_removes() {
        let (state, repo, _owner, dev) = seed_workspace_with_members().await;
        let app = crate::api::api_router().with_state(state.clone());

        let resp = put_override(app, &repo, &dev, "Viewer").await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        // Same (repo, user) again: update, not a duplicate.
        let app = crate::api::api_router().with_state(state.clone());
        let resp = put_override(app, &repo, &dev, "Developer").await;
        assert_eq!(resp.status(), StatusCode::OK);

        let listed = state
            .repo_access_overrides
            .list_by_repo(&Id::new(&repo))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1, "one override per (repo, user)");
        assert_eq!(listed[0].role, WorkspaceRole::Developer);

        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/{repo}/access-overrides/{dev}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(state
            .repo_access_overrides
            .find_by_repo_and_user(&Id::new(&repo), &Id::new(&dev))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn override_on_non_member_is_rejected() {
        let (state, repo, _owner, _dev) = seed_workspace_with_members().await;
        let app = crate::api::api_router().with_state(state);
        let resp = put_override(app, &repo, "stranger", "Viewer").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
