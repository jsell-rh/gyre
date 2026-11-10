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
///
/// Fail-closed on an unresolvable scope: a repo whose workspace lookup
/// errors or comes back empty is rejected (NotFound), never passed
/// through — the tenant gate cannot be evaluated, and this repo's
/// standards treat that as denial (same containment as git_http's
/// tenant-scoped `resolve_repo_by_slug`).
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
    if !auth.is_system_principal() {
        let ws = state
            .workspaces
            .find_by_id(&repo.workspace_id)
            .await
            .map_err(ApiError::Internal)?
            .ok_or_else(|| ApiError::NotFound(format!("workspace for repo {repo_id} not found")))?;
        if ws.tenant_id.to_string() != auth.tenant_id {
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
#[cfg(test)]
mod tests {
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::{Repository, User, Workspace, WorkspaceMembership, WorkspaceRole};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Build the PRODUCTION request pipeline for authorization tests: the
    /// api router with the ABAC middleware layered over it and the built-in
    /// policies seeded, exactly as `build_router` does in lib.rs. Tests
    /// that build `api_router()` bare bypass policy evaluation entirely —
    /// they cannot see a route whose intended managers are default-denied
    /// by the middleware before the handler ever runs.
    fn production_app(state: &Arc<crate::AppState>) -> Router {
        crate::abac_middleware::init_resolver();
        crate::api::api_router()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::abac_middleware::abac_middleware,
            ))
            .with_state(state.clone())
    }

    /// The fixture: tenant t-1, one workspace with one repo, and two user
    /// principals with API keys —
    ///
    /// - the manager: a workspace Admin member whose GLOBAL role is a plain
    ///   Developer (no platform Admin, no TenantAdmin). This is exactly the
    ///   principal user-management.md §Repo Access says may manage per-repo
    ///   overrides ("Admin: Full (read, write, admin, manage gates)"), and
    ///   exactly the one a bare-router harness cannot speak for: only the
    ///   ABAC path (subject.workspace_role resolved from the repo's
    ///   workspace) can admit them.
    /// - the target: a workspace Developer member, the subject of the
    ///   override.
    struct Fixture {
        app: Router,
        state: Arc<crate::AppState>,
        repo_id: String,
        target_id: String,
        manager_key: String,
        target_key: String,
    }

    async fn seed(target_role: WorkspaceRole) -> Fixture {
        let state = crate::mem::test_state();
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed builtin policies");

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

        // A user of tenant t-1 with an API key and a workspace membership.
        async fn member(
            state: &Arc<crate::AppState>,
            ws_id: &str,
            workspace_role: WorkspaceRole,
            key: &'static str,
            now: u64,
        ) -> String {
            let user_id = uuid::Uuid::new_v4().to_string();
            let mut user = User::new(
                Id::new(user_id.clone()),
                format!("member-ext-{user_id}"),
                format!("member-{user_id}"),
                now,
            );
            user.tenant_id = Some(Id::new("t-1"));
            // Global role stays a plain Member with the Developer platform
            // role — the workspace membership, not the global role, is what
            // must carry the override-management capability.
            user.global_role = gyre_domain::GlobalRole::Member;
            user.roles = vec![gyre_domain::UserRole::Developer];
            state.users.create(&user).await.unwrap();

            let mut membership = WorkspaceMembership::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                Id::new(user_id.clone()),
                Id::new(ws_id),
                workspace_role,
                Id::new("system"),
                now,
            );
            membership.accept(now);
            state
                .workspace_memberships
                .create(&membership)
                .await
                .unwrap();
            state
                .api_keys
                .create(
                    &crate::auth::hash_api_key(key),
                    &Id::new(user_id.clone()),
                    "test",
                )
                .await
                .unwrap();
            user_id
        }

        let _manager_id = member(&state, &ws_id, WorkspaceRole::Admin, "ws-admin-key", now).await;
        let target_id = member(&state, &ws_id, target_role, "ws-target-key", now).await;

        let app = production_app(&state);
        Fixture {
            app,
            state,
            repo_id,
            target_id,
            manager_key: "ws-admin-key".to_string(),
            target_key: "ws-target-key".to_string(),
        }
    }

    fn put_override(f: &Fixture, user_id: &str, role: &str) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(format!(
                "/api/v1/repos/{}/access-overrides/{user_id}",
                f.repo_id
            ))
            .header("Authorization", format!("Bearer {}", f.manager_key))
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::json!({"role": role}).to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn workspace_admin_manages_overrides_through_production_abac() {
        // user-management.md §Repo Access gives the workspace Admin role
        // repo `admin`. This test runs the PRODUCTION pipeline (api router
        // + abac_middleware + builtin policies, as build_router layers
        // them): a workspace Admin member whose global role is a plain
        // Developer must reach the handler (ABAC allow via
        // builtin-repo-access-override-management resolving
        // subject.workspace_role from the repo's workspace) and create the
        // override. Regressions that leave subject.workspace_role
        // unresolved on repo-addressed routes default-deny this principal
        // with a 403 before the handler runs.
        let f = seed(WorkspaceRole::Developer).await;

        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Viewer"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "a workspace Admin member must manage per-repo overrides through the ABAC layer"
        );

        // The ABAC decision for the manager's request was Allow — the 201
        // came through policy evaluation, not around it.
        let decisions = f
            .state
            .policies
            .list_decisions(None, Some("repo"), 50)
            .await
            .unwrap();
        let put_decision = decisions
            .iter()
            .find(|d| d.action == "admin" && d.decision == gyre_domain::PolicyEffect::Allow)
            .expect("an Allow decision for the repo admin action");
        assert_eq!(
            put_decision.matched_policy.as_ref().map(|p| p.as_str()),
            Some("builtin-repo-access-override-management"),
            "the workspace Admin's allow must come from the override-management policy"
        );

        // DELETE lifts the restriction through the same path.
        let resp = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/repos/{}/access-overrides/{}",
                        f.repo_id, f.target_id
                    ))
                    .header("Authorization", format!("Bearer {}", f.manager_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn non_admin_member_cannot_manage_overrides_through_production_abac() {
        // A Developer workspace member (global Developer) is denied by the
        // ABAC layer itself: the repo `admin` action matches no policy for
        // them (builtin-developer-write-access covers read/write/spawn/...,
        // not admin; the override-management policy requires
        // subject.workspace_role In [Owner, Admin]).
        let mut f = seed(WorkspaceRole::Developer).await;
        f.manager_key = f.target_key.clone();

        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Viewer"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a Developer member must be denied override management by the ABAC layer"
        );
    }

    #[tokio::test]
    async fn override_restricts_effective_permissions() {
        // user-management.md §Repo Access: a per-repo override restricts a
        // Developer to Viewer on one repo — the effective permission set
        // collapses to read-only while the workspace role stays Developer.
        let f = seed(WorkspaceRole::Developer).await;

        // Before the override: the target reads their own access
        // (Developer permissions per the spec table).
        let resp = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/repos/{}/access/{}",
                        f.repo_id, f.target_id
                    ))
                    .header("Authorization", format!("Bearer {}", f.target_key))
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

        // The workspace Admin restricts the Developer to Viewer on this
        // repo (through the production ABAC pipeline).
        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Viewer"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        assert_eq!(created["restricted_role"], "Viewer");

        // The effective access view now shows the restricted role and a
        // read-only permission set.
        let resp = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/repos/{}/access/{}",
                        f.repo_id, f.target_id
                    ))
                    .header("Authorization", format!("Bearer {}", f.target_key))
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
        let resp = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/repos/{}/access-overrides/{}",
                        f.repo_id, f.target_id
                    ))
                    .header("Authorization", format!("Bearer {}", f.manager_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            f.state
                .repo_access_overrides
                .find_by_repo_and_user(&Id::new(&f.repo_id), &Id::new(&f.target_id))
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
        let f = seed(WorkspaceRole::Viewer).await;

        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Developer"))
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
        let f = seed(WorkspaceRole::Developer).await;

        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Viewer"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, &f.target_id, "Viewer"))
            .await
            .unwrap();
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
        let f = seed(WorkspaceRole::Developer).await;
        let resp = f
            .app
            .clone()
            .oneshot(put_override(&f, "nobody-home", "Viewer"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "restricting a non-member must be a 404"
        );
    }

    #[tokio::test]
    async fn dangling_workspace_scope_is_fail_closed() {
        // A repo whose workspace row is missing must not pass the tenant
        // gate: the scope cannot be evaluated, and an unevaluable tenant
        // gate is denial (fail-closed), never pass-through.
        let f = seed(WorkspaceRole::Developer).await;

        // Point the repo at a workspace that does not exist.
        let dangling_repo = Repository::new(
            Id::new("repo-dangling"),
            Id::new("ws-no-such-workspace"),
            "dangling".to_string(),
            "/tmp/gyre-test/dangling",
            1_700_000_000,
        );
        f.state.repos.create(&dangling_repo).await.unwrap();

        let resp = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/repos/{}/access/{}",
                        dangling_repo.id, f.target_id
                    ))
                    .header("Authorization", format!("Bearer {}", f.manager_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "a repo with an unresolvable workspace scope must be denied, not passed through"
        );
    }
}
