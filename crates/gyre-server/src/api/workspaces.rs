use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{trust_policies_for_level, BudgetConfig, TrustLevel, UserRole, Workspace};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    /// Optional override for Admin callers only. Non-admin callers have
    /// tenant_id derived from their auth context and this field is ignored.
    pub tenant_id: Option<String>,
    pub name: String,
    /// Slug is optional — auto-derived from name (lowercase, spaces → hyphens,
    /// non-alphanumeric stripped) when omitted. Callers may supply an explicit slug.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub trust_level: Option<String>,
    pub llm_model: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub trust_level: Option<String>,
    pub llm_model: Option<String>,
    /// Set to a compute target ID to bind agent spawning, or null to clear.
    pub compute_target_id: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct ListWorkspacesQuery {
    pub tenant_id: Option<String>,
    /// Filter by workspace slug (for cross-workspace spec link resolution).
    pub slug: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub trust_level: String,
    pub llm_model: Option<String>,
    pub created_at: u64,
    pub compute_target_id: Option<String>,
}

impl From<Workspace> for WorkspaceResponse {
    fn from(ws: Workspace) -> Self {
        Self {
            id: ws.id.to_string(),
            tenant_id: ws.tenant_id.to_string(),
            name: ws.name,
            slug: ws.slug,
            description: ws.description,
            budget: ws.budget,
            max_repos: ws.max_repos,
            max_agents_per_repo: ws.max_agents_per_repo,
            trust_level: ws.trust_level.to_string(),
            llm_model: ws.llm_model,
            created_at: ws.created_at,
            compute_target_id: ws.compute_target_id.map(|id| id.to_string()),
        }
    }
}

#[derive(Deserialize)]
pub struct AddRepoRequest {
    pub repo_id: String,
}

#[derive(Serialize)]
pub struct WorkspaceRepoEntry {
    pub repo_id: String,
}

pub async fn create_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceResponse>), ApiError> {
    // Derive tenant_id from auth context. Admin callers may supply a
    // tenant_id override in the request body; all others are bound to
    // their authenticated tenant scope. Using role check (not agent_id
    // string match) prevents spoofing by a JWT user named "system".
    let tenant_id = if auth.roles.contains(&UserRole::Admin) {
        req.tenant_id.unwrap_or(auth.tenant_id)
    } else {
        auth.tenant_id
    };
    let now = now_secs();
    let slug = req.slug.unwrap_or_else(|| {
        req.name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    });
    let mut ws = Workspace::new(new_id(), Id::new(&tenant_id), req.name, slug, now);
    ws.description = req.description;
    ws.budget = req.budget;
    ws.max_repos = req.max_repos;
    ws.max_agents_per_repo = req.max_agents_per_repo;
    if let Some(tl) = req.trust_level {
        ws.trust_level = TrustLevel::from_db_str(&tl);
    }
    ws.llm_model = req.llm_model;
    state.workspaces.create(&ws).await?;
    Ok((StatusCode::CREATED, Json(WorkspaceResponse::from(ws))))
}

pub async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(q): Query<ListWorkspacesQuery>,
) -> Result<Json<Vec<WorkspaceResponse>>, ApiError> {
    // Admin callers may filter by an explicit tenant_id query param.
    // All other callers are restricted to their own tenant scope.
    let tenant_id = if auth.roles.contains(&UserRole::Admin) {
        q.tenant_id.unwrap_or(auth.tenant_id)
    } else {
        auth.tenant_id
    };
    let workspaces = state
        .workspaces
        .list_by_tenant(&Id::new(&tenant_id))
        .await?;
    // Optional ?slug= filter for cross-workspace spec link resolution.
    let workspaces: Vec<_> = if let Some(slug) = q.slug {
        workspaces
            .into_iter()
            .filter(|ws| ws.slug == slug)
            .collect()
    } else {
        workspaces
    };
    Ok(Json(
        workspaces
            .into_iter()
            .map(WorkspaceResponse::from)
            .collect(),
    ))
}

pub async fn get_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    // Enforce tenant ownership — non-Admin callers can only access their own tenant's workspaces.
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {id} not found")));
    }
    Ok(Json(WorkspaceResponse::from(ws)))
}

pub async fn update_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let mut ws = state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {id} not found")));
    }
    if let Some(name) = req.name {
        ws.name = name;
    }
    if let Some(desc) = req.description {
        ws.description = Some(desc);
    }
    if let Some(budget) = req.budget {
        ws.budget = Some(budget);
    }
    if let Some(max_repos) = req.max_repos {
        ws.max_repos = Some(max_repos);
    }
    if let Some(max_agents) = req.max_agents_per_repo {
        ws.max_agents_per_repo = Some(max_agents);
    }
    let trust_changed = if let Some(tl) = req.trust_level {
        let new_trust = TrustLevel::from_db_str(&tl);
        let changed = new_trust != ws.trust_level;
        ws.trust_level = new_trust;
        changed
    } else {
        false
    };
    if let Some(model) = req.llm_model {
        ws.llm_model = Some(model);
    }
    if let Some(ct_id_value) = req.compute_target_id {
        ws.compute_target_id = match ct_id_value {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(Id::new(s)),
            _ => ws.compute_target_id,
        };
    }
    state.workspaces.update(&ws).await?;

    // Apply trust preset ABAC policies as a side effect of trust level change.
    // When transitioning TO Custom, preserve existing trust: policies as the
    // starting point for user-managed ABAC (HSI §2). On all other transitions,
    // delete workspace-scoped trust: policies and seed the new preset.
    if trust_changed {
        let is_now_custom = matches!(ws.trust_level, TrustLevel::Custom);
        if !is_now_custom {
            state
                .policies
                .delete_by_name_prefix_and_scope_id("trust:", ws.id.as_str())
                .await
                .map_err(ApiError::Internal)?;
        }
        for policy in trust_policies_for_level(&ws.trust_level, ws.id.as_str(), &auth.agent_id) {
            state.policies.create(&policy).await.map_err(|e| {
                ApiError::Internal(anyhow::anyhow!(
                    "Trust level transition failed — policies could not be created: {e}"
                ))
            })?;
        }
    }

    Ok(Json(WorkspaceResponse::from(ws)))
}

pub async fn delete_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {id} not found")));
    }
    state.workspaces.delete(&Id::new(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_repo_to_workspace(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(ws_id): Path<String>,
    Json(req): Json<AddRepoRequest>,
) -> Result<StatusCode, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {ws_id} not found")));
    }
    let mut repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;
    repo.workspace_id = Id::new(&ws_id);
    state.repos.update(&repo).await?;
    let mut repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &ws_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    repo_ids.push(req.repo_id);
    let json = serde_json::to_string(&repo_ids).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("workspace_repos", &ws_id, json)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_workspace_repos(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(ws_id): Path<String>,
) -> Result<Json<Vec<WorkspaceRepoEntry>>, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {ws_id} not found")));
    }
    let repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &ws_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(Json(
        repo_ids
            .into_iter()
            .map(|repo_id| WorkspaceRepoEntry { repo_id })
            .collect(),
    ))
}

/// Response entry for GET /api/v1/workspaces/:workspace_id/presence.
#[derive(Serialize)]
pub struct PresenceEntryResponse {
    pub user_id: String,
    pub session_id: String,
    pub view: String,
    pub last_seen: u64,
}

/// GET /api/v1/workspaces/:workspace_id/presence
///
/// Returns the current in-memory presence map for the given workspace.
/// Used by clients on reconnection to populate the initial presence state.
pub async fn get_workspace_presence(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<PresenceEntryResponse>>, ApiError> {
    // Verify workspace exists and caller has access.
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&workspace_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!(
            "workspace {workspace_id} not found"
        )));
    }

    let map = state.presence.read().await;
    let entries: Vec<PresenceEntryResponse> = map
        .iter()
        .filter(|(_, entry)| entry.workspace_id == workspace_id)
        .map(|((user_id, session_id), entry)| PresenceEntryResponse {
            user_id: user_id.clone(),
            session_id: session_id.clone(),
            view: entry.view.clone(),
            last_seen: entry.timestamp,
        })
        .collect();

    Ok(Json(entries))
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_and_list_workspaces() {
        let app = app();
        let body = serde_json::json!({
            "tenant_id": "t1",
            "name": "Platform Team",
            "slug": "platform-team"
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["name"], "Platform Team");
        assert_eq!(created["slug"], "platform-team");
        assert_eq!(created["tenant_id"], "t1");

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces?tenant_id=t1")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_workspace_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/nonexistent")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_workspace() {
        let app = app();
        let body = serde_json::json!({ "tenant_id": "t1", "name": "Old", "slug": "old" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update_body = serde_json::json!({ "name": "New Name" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["name"], "New Name");
    }

    #[tokio::test]
    async fn delete_workspace() {
        let app = app();
        let body = serde_json::json!({ "tenant_id": "t1", "name": "Del", "slug": "del" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }

    // -- Tenant-isolation security tests (NEW-17) -----------------------------

    /// Non-admin callers cannot override tenant_id via the request body.
    /// The workspace must be created under the caller's auth tenant, not
    /// the attacker-supplied value.
    #[tokio::test]
    async fn create_workspace_non_admin_tenant_override_ignored() {
        let state = test_state();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-sec-1", "agent-tok-1".to_string())
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "tenant_id": "evil-tenant",
            "name": "Malicious",
            "slug": "malicious"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer agent-tok-1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        // Agent token has tenant_id "default" — the supplied "evil-tenant" must be ignored.
        assert_eq!(created["tenant_id"], "default");
        assert_ne!(created["tenant_id"], "evil-tenant");
    }

    /// Non-admin callers cannot filter list by a different tenant via query param.
    /// Supplying ?tenant_id=other-tenant must be silently ignored; they only see
    /// their own tenant's workspaces.
    #[tokio::test]
    async fn list_workspaces_non_admin_cannot_override_tenant_filter() {
        let state = test_state();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-sec-2", "agent-tok-2".to_string())
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        // Create a workspace in "other-tenant" as admin (system token).
        let body = serde_json::json!({
            "tenant_id": "other-tenant",
            "name": "Other",
            "slug": "other"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Non-admin agent tries to list "other-tenant" workspaces.
        // The query param must be ignored; only their own tenant ("default") is queried.
        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces?tenant_id=other-tenant")
                    .header("authorization", "Bearer agent-tok-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        // Should be empty — the agent is scoped to "default", not "other-tenant".
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    // -- Tenant-isolation security tests (NEW-18) -----------------------------

    /// Non-admin callers cannot read a workspace belonging to another tenant via UUID.
    /// get_workspace must return 404 (not leak existence) for cross-tenant access.
    #[tokio::test]
    async fn get_workspace_cross_tenant_returns_404() {
        let state = test_state();
        // Register a non-admin agent token (tenant "default").
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agent-new18-1",
                "agent-tok-new18-1".to_string(),
            )
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        // Admin creates a workspace in "other-tenant".
        let body = serde_json::json!({
            "tenant_id": "other-tenant",
            "name": "OtherWs",
            "slug": "other-ws"
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token") // Admin
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        let other_id = created["id"].as_str().unwrap().to_string();

        // Non-admin agent (tenant "default") tries to GET the other-tenant workspace by UUID.
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{other_id}"))
                    .header("authorization", "Bearer agent-tok-new18-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Must return 404, not 200 — cross-tenant UUID access denied.
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    /// Non-admin callers cannot delete a workspace belonging to another tenant.
    #[tokio::test]
    async fn delete_workspace_cross_tenant_returns_404() {
        let state = test_state();
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agent-new18-2",
                "agent-tok-new18-2".to_string(),
            )
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "tenant_id": "victim-tenant",
            "name": "VictimWs",
            "slug": "victim-ws"
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let victim_id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/workspaces/{victim_id}"))
                    .header("authorization", "Bearer agent-tok-new18-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NOT_FOUND);
    }

    // -- Trust level transition tests (S1.3) ----------------------------------

    /// Transitioning to Supervised creates the trust:require-human-mr-review policy.
    #[tokio::test]
    async fn trust_transition_to_supervised_creates_trust_policy() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create a workspace.
        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let ws_id = body_json(create_resp).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Transition to Supervised trust.
        let update = serde_json::json!({ "trust_level": "Supervised" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["trust_level"], "Supervised");

        // The trust:require-human-mr-review policy must now exist.
        let policies = state.policies.list().await.unwrap();
        let trust_policy = policies
            .iter()
            .find(|p| p.name == "trust:require-human-mr-review");
        assert!(
            trust_policy.is_some(),
            "trust:require-human-mr-review must be created on Supervised transition"
        );

        // Transition to Guided — trust: policies must be deleted.
        let update2 = serde_json::json!({ "trust_level": "Guided" });
        let update_resp2 = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp2.status(), StatusCode::OK);
        let updated2 = body_json(update_resp2).await;
        assert_eq!(updated2["trust_level"], "Guided");

        let policies2 = state.policies.list().await.unwrap();
        let trust_policy2 = policies2.iter().find(|p| p.name.starts_with("trust:"));
        assert!(
            trust_policy2.is_none(),
            "trust: policies must be deleted on transition away from Supervised"
        );
    }
}
