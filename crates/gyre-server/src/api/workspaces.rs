use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    trust_policies_for_level, BudgetConfig, TrustLevel, UserRole, Workspace, WorkspaceMembership,
    WorkspaceRole,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::repos::RepoResponse;
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
    /// Raw deserializer keeps an explicit JSON `null` distinct from an
    /// absent field: serde collapses `null` to `None` inside
    /// `Option<Value>`, which would make a clear a silent no-op.
    #[serde(default, deserialize_with = "crate::api::deserialize_present")]
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
        // Strict parse of caller input: a typo must 400, not silently
        // coerce to the Supervised default (which would seed trust
        // policies the caller never asked for). `from_db_str`'s
        // fallback is for stored rows only.
        ws.trust_level = TrustLevel::parse(&tl).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "invalid trust_level '{tl}'; must be Supervised, Guided, Autonomous, or Custom"
            ))
        })?;
    }
    ws.llm_model = req.llm_model;
    // Seed the trust preset ABAC policies atomically with the workspace row so a
    // Supervised workspace is never persisted (or returned) without its
    // `trust:require-human-mr-review` Deny policy — otherwise the merge processor
    // would be unblocked in a workspace the user believes is Supervised (F1/F2).
    // `delete_trust_policies = false`: a brand-new workspace has no prior policies.
    let seed_policies = trust_policies_for_level(&ws.trust_level, ws.id.as_str(), &auth.agent_id);
    state
        .workspaces
        .apply_trust_transition(&ws, false, &seed_policies)
        .await
        .map_err(ApiError::Internal)?;
    // apply_trust_transition persists policies outside the PolicyRepository
    // port (atomicity) — invalidate the policy-set cache so the ABAC engine
    // sees the seeded trust policies immediately (policy_cache.rs contract).
    state.policy_cache.invalidate();

    // Seed the creator's Owner membership (user-management.md §Workspace
    // Membership): a workspace created by a user principal is owned by its
    // creator from birth, so it is never falsely flagged ownerless by the
    // orphaned-workspace job (§Ownership Transfer & Reclamation frames the
    // flag as "when a workspace Owner leaves"). Only seeded when the
    // creator is a user of the workspace's tenant — cross-tenant
    // memberships are not supported (Admin callers may create workspaces
    // in other tenants via the tenant_id override; those are owned by the
    // first invited Owner). System/agent principals have no user identity
    // and seed nothing.
    if let Some(user_id) = &auth.user_id {
        let user = state.users.find_by_id(user_id).await?;
        if user.as_ref().and_then(|u| u.tenant_id.as_ref()) == Some(&ws.tenant_id) {
            let mut membership = WorkspaceMembership::new(
                new_id(),
                user_id.clone(),
                ws.id.clone(),
                WorkspaceRole::Owner,
                user_id.clone(),
                now,
            );
            // The creator accepts their own ownership immediately — the
            // membership is active from creation, not a pending invite.
            membership.accept(now);
            state
                .workspace_memberships
                .create(&membership)
                .await
                .map_err(ApiError::Internal)?;
        }
    }
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
        // Strict parse of caller input (400 on typo) — a misspelled level
        // must not fall through to `from_db_str`'s Supervised fallback,
        // which would silently transition the workspace and rewrite its
        // trust: policies while returning 200.
        let new_trust = TrustLevel::parse(&tl).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "invalid trust_level '{tl}'; must be Supervised, Guided, Autonomous, or Custom"
            ))
        })?;
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
            serde_json::Value::String(s) => {
                // agent-runtime.md §3: "Workspace selects ONE compute target
                // from the tenant's list" — the ID must resolve to an
                // existing target in THIS workspace's tenant. Rejecting
                // here prevents binding a workspace to another tenant's
                // target (cross-tenant leak) or to a stale ID that would
                // silently fall through the spawn-time fallback chain.
                let target = state
                    .compute_targets
                    .get_by_id(&Id::new(&s))
                    .await?
                    .ok_or_else(|| {
                        ApiError::InvalidInput(format!("compute target {s} not found"))
                    })?;
                if target.tenant_id != ws.tenant_id {
                    // Same shape as the compute-targets handlers: a
                    // cross-tenant ID is indistinguishable from a missing
                    // one — do not confirm its existence.
                    return Err(ApiError::InvalidInput(format!(
                        "compute target {s} not found"
                    )));
                }
                Some(target.id)
            }
            _ => {
                return Err(ApiError::InvalidInput(
                    "compute_target_id must be a string or null".to_string(),
                ))
            }
        };
    }
    // When the trust level changes, write the workspace row AND its `trust:`
    // preset policies in a single atomic transaction (F3): `apply_trust_transition`
    // upserts the full workspace row plus policies, so we must NOT also call
    // `workspaces.update` — that would write `trust_level` twice and open a
    // partial-update window. When transitioning TO Custom, existing `trust:`
    // policies are preserved (`delete_trust_policies = false`) as the starting
    // point for user-managed ABAC (HSI §2); all other transitions replace them.
    // A failed transition rolls back and maps to 409 Conflict.
    if trust_changed {
        let is_now_custom = matches!(ws.trust_level, TrustLevel::Custom);
        let new_policies =
            trust_policies_for_level(&ws.trust_level, ws.id.as_str(), &auth.agent_id);
        state
            .workspaces
            .apply_trust_transition(&ws, !is_now_custom, &new_policies)
            .await
            .map_err(|_| {
                // Verbatim HSI §2 message — the UI surfaces this string.
                ApiError::Conflict(
                    "Trust level transition failed — policies could not be created".to_string(),
                )
            })?;
        // apply_trust_transition persists policies outside the
        // PolicyRepository port (atomicity) — invalidate the policy-set
        // cache so the ABAC engine sees the transitioned trust policies
        // immediately (policy_cache.rs contract). Without this, a
        // Supervised workspace's merge-hold Deny could stay invisible to
        // merge-time evaluation until an unrelated policy write.
        state.policy_cache.invalidate();
    } else {
        // No trust change — only the non-trust workspace fields need updating.
        state.workspaces.update(&ws).await?;
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
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_workspace_repos(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(ws_id): Path<String>,
) -> Result<Json<Vec<RepoResponse>>, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(&ws_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {ws_id} not found")));
    }
    let repos = state.repos.list_by_workspace(&Id::new(&ws_id)).await?;
    let mut responses = Vec::with_capacity(repos.len());
    for repo in repos {
        responses.push(super::repos::repo_response_with_clone_url(&state, repo).await);
    }
    Ok(Json(responses))
}

/// Response entry for GET /api/v1/workspaces/:workspace_id/presence.
#[derive(Serialize)]
pub struct PresenceEntryResponse {
    pub user_id: String,
    pub session_id: String,
    pub view: String,
    pub last_seen: u64,
    /// Entity the session is actively editing, e.g. `"spec:specs/system/x.md"`
    /// (HSI §7). Absent when the session is only navigating a view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editing_entity: Option<String>,
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
            editing_entity: entry.editing_entity.clone(),
        })
        .collect();

    Ok(Json(entries))
}

// ---------------------------------------------------------------------------
// Workspace merge policy (meta-spec-reconciliation.md §9)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct MergePolicyResponse {
    pub require_current_meta_spec_set: bool,
}

impl From<crate::merge_policy::WorkspaceMergePolicy> for MergePolicyResponse {
    fn from(p: crate::merge_policy::WorkspaceMergePolicy) -> Self {
        Self {
            require_current_meta_spec_set: p.require_current_meta_spec_set,
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateMergePolicyRequest {
    pub require_current_meta_spec_set: Option<bool>,
}

/// GET /api/v1/workspaces/:id/merge-policy
///
/// Returns the workspace merge policy — the §9 strict-mode switch for the
/// meta-spec drift merge gate. Default: warn only.
pub async fn get_merge_policy(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<MergePolicyResponse>, ApiError> {
    let workspace_id = Id::new(&id);

    // Verify workspace exists and the caller's tenant owns it.
    let ws = state
        .workspaces
        .find_by_id(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if !auth.roles.contains(&UserRole::Admin) && ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {id} not found")));
    }

    let policy = crate::merge_policy::get_merge_policy(&state, &workspace_id).await?;
    Ok(Json(policy.into()))
}

/// PUT /api/v1/workspaces/:id/merge-policy (Admin only)
///
/// Update the workspace merge policy (partial update). Admin-only: the
/// strict-mode flag converts the §9 meta-spec drift warning into a hard
/// merge block — a governance control that determines whether in-flight MRs
/// can merge during reconciliation, so non-Admin callers (Developers,
/// Agents) must not be able to flip it (same NEW-26 rule as the meta-spec-set
/// binding).
pub async fn set_merge_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateMergePolicyRequest>,
) -> Result<Json<MergePolicyResponse>, ApiError> {
    // Admin-only: see doc comment above.
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may update workspace merge policy".to_string(),
        ));
    }

    let workspace_id = Id::new(&id);

    // Verify workspace exists and the caller's tenant owns it.
    let ws = state
        .workspaces
        .find_by_id(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if ws.tenant_id != Id::new(&auth.tenant_id) {
        return Err(ApiError::NotFound(format!("workspace {id} not found")));
    }

    // Apply partial update.
    let mut policy = crate::merge_policy::get_merge_policy(&state, &workspace_id).await?;
    if let Some(require) = req.require_current_meta_spec_set {
        policy.require_current_meta_spec_set = require;
    }
    crate::merge_policy::set_merge_policy(&state, &workspace_id, &policy).await?;

    Ok(Json(policy.into()))
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
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
    async fn create_workspace_seeds_owner_membership_for_creator() {
        // user-management.md §Workspace Membership + §Ownership Transfer &
        // Reclamation: a workspace created by a user principal is owned by
        // its creator from birth (active Owner membership), so the
        // orphaned-workspace job never flags a freshly created workspace —
        // the flag is for owner DEPARTURE ("when a workspace Owner
        // leaves"), not for workspaces that simply haven't invited an
        // Owner yet.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let mut creator = gyre_domain::User::new(
            Id::new("creator-1"),
            "ext-creator-1",
            "creator-1",
            1_700_000_000,
        );
        creator.tenant_id = Some(Id::new("t1"));
        creator.global_role = gyre_domain::GlobalRole::Member;
        creator.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&creator).await.unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key("creator-key-1"),
                &creator.id,
                "test",
            )
            .await
            .unwrap();

        let body = serde_json::json!({
            "name": "Creator Owned",
            "slug": "creator-owned"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer creator-key-1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created = body_json(resp).await;
        let ws_id = created["id"].as_str().unwrap().to_string();

        let membership = state
            .workspace_memberships
            .find_by_user_and_workspace(&creator.id, &Id::new(&ws_id))
            .await
            .unwrap()
            .expect("the creator must hold a membership in the workspace they created");
        assert_eq!(membership.role, gyre_domain::WorkspaceRole::Owner);
        assert!(
            membership.accepted && membership.accepted_at.is_some(),
            "the creator's Owner membership is active immediately, not a pending invite"
        );

        // The orphaned-workspace job does not flag the creator-owned
        // workspace (an active Owner exists).
        crate::workspace_ownership::run_once(&state).await.unwrap();
        let flagged = crate::workspace_ownership::orphaned_workspaces_with(&state, 0)
            .await
            .unwrap();
        assert!(
            flagged.iter().all(|w| w.workspace_id != ws_id),
            "a workspace owned by its creator must never be flagged orphaned"
        );
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

        // Create a Guided workspace (entity default is Supervised since
        // TASK-077; pin Guided so the PUT below exercises a real transition).
        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w", "trust_level": "Guided" });
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

    /// A failed (rolled-back) trust transition maps to HTTP 409 Conflict.
    ///
    /// Uses a workspace repo whose `apply_trust_transition` always errors,
    /// simulating a DB transaction rollback. The workspace is seeded directly
    /// (bypassing the create handler, which also goes through the transition),
    /// then a PUT that changes the trust level must return 409.
    #[tokio::test]
    async fn update_workspace_trust_transition_failure_returns_409() {
        let state = crate::mem::test_state_failing_trust();
        // Seed a Guided workspace directly. Trust level is pinned explicitly:
        // the entity default is Supervised (HSI §2) and the test needs a
        // Guided → Supervised transition below.
        let mut ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-fail-1"),
            gyre_common::Id::new("t1"),
            "FailWs",
            "fail-ws",
            0,
        );
        ws.trust_level = gyre_domain::TrustLevel::Guided;
        state.workspaces.create(&ws).await.unwrap();
        let app = crate::api::api_router().with_state(state.clone());

        // Changing Guided -> Supervised triggers a trust transition, which the
        // failing repo rejects; the handler must surface a 409 Conflict.
        let update = serde_json::json!({ "trust_level": "Supervised" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-fail-1")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Rollback: the failed transition must leave the workspace unchanged
        // (still Guided) and must not have partially applied trust policies.
        let ws_after = state
            .workspaces
            .find_by_id(&gyre_common::Id::new("ws-fail-1"))
            .await
            .unwrap()
            .expect("workspace still exists after failed transition");
        assert_eq!(ws_after.trust_level, gyre_domain::TrustLevel::Guided);

        let policies_after = state.policies.list().await.unwrap();
        assert!(
            !policies_after.iter().any(|p| p.name.starts_with("trust:")
                && p.scope_id.as_ref().map(|i| i.as_str()) == Some("ws-fail-1")),
            "failed transition must not partially apply trust: policies"
        );
    }

    /// New workspaces default to Supervised trust (HSI §2) and are seeded
    /// with the `trust:require-human-mr-review` policy atomically at create.
    #[tokio::test]
    async fn create_workspace_defaults_to_supervised_and_seeds_trust_policy() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w" });
        let create_resp = app
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
        assert_eq!(created["trust_level"], "Supervised");

        // The Supervised trust preset must be seeded with the workspace row.
        let policies = state.policies.list().await.unwrap();
        assert!(
            policies
                .iter()
                .any(|p| p.name == "trust:require-human-mr-review"),
            "Supervised default must seed trust:require-human-mr-review"
        );
    }

    /// An explicit trust_level in the create body is honored and its preset
    /// seeded accordingly (Guided → no trust: policies).
    #[tokio::test]
    async fn create_workspace_explicit_guided_has_no_trust_policies() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w", "trust_level": "Guided" });
        let create_resp = app
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
        assert_eq!(created["trust_level"], "Guided");

        let policies = state.policies.list().await.unwrap();
        assert!(
            !policies.iter().any(|p| p.name.starts_with("trust:")),
            "Guided preset must not seed trust: policies"
        );
    }

    /// §9 workspace merge policy API: default is warn-only, a Developer-role
    /// caller cannot flip the strict-mode flag, and an Admin can
    /// (meta-spec-reconciliation.md §9, task-159).
    #[tokio::test(flavor = "multi_thread")]
    async fn merge_policy_default_admin_only_update_and_roundtrip() {
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        let _ = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        // Create a workspace with the admin static token.
        let ws_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"ws-mp","slug":"ws-mp"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        // Unknown workspace → 404.
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/does-not-exist/merge-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Default policy: warn only.
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{ws_id}/merge-policy"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let policy = body_json(resp).await;
        assert_eq!(policy["require_current_meta_spec_set"], false);

        // A Developer-role JWT must not flip the strict-mode flag.
        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/merge-policy"))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"require_current_meta_spec_set": true}"#.to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // The rejected update changed nothing.
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{ws_id}/merge-policy"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy = body_json(resp).await;
        assert_eq!(policy["require_current_meta_spec_set"], false);

        // Admin (static token) enables strict mode.
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}/merge-policy"))
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"require_current_meta_spec_set": true}"#.to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let policy = body_json(resp).await;
        assert_eq!(policy["require_current_meta_spec_set"], true);

        // GET reflects the persisted strict mode.
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{ws_id}/merge-policy"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let policy = body_json(resp).await;
        assert_eq!(policy["require_current_meta_spec_set"], true);
    }

    // -- Compute target binding (agent-runtime.md §3) --------------------------

    /// Binding a workspace to a compute target in the same tenant succeeds
    /// and round-trips through the workspace response (spec §3: "Workspace
    /// selects ONE compute target from the tenant's list").
    #[tokio::test]
    async fn update_workspace_binds_compute_target_in_tenant() {
        let state = crate::mem::test_state();
        // Seed a compute target in tenant t1 directly via the repository.
        let ct = gyre_domain::ComputeTargetEntity::new(
            gyre_common::Id::new("ct-t1-1"),
            gyre_common::Id::new("t1"),
            "local-docker".to_string(),
            gyre_domain::ComputeTargetType::Container,
            100,
        );
        state.compute_targets.create(&ct).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "tenant_id": "t1", "name": "CtWs", "slug": "ct-ws" });
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
        let id = created["id"].as_str().unwrap().to_string();

        let update = serde_json::json!({ "compute_target_id": "ct-t1-1" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = body_json(resp).await;
        assert_eq!(updated["compute_target_id"], "ct-t1-1");

        // Clearing with null also round-trips.
        let clear = serde_json::json!({ "compute_target_id": null });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&clear).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = body_json(resp).await;
        assert!(updated["compute_target_id"].is_null());
    }

    /// A compute target from ANOTHER tenant must be rejected: the spawn-time
    /// fallback chain would otherwise never resolve it (workspace tenant ≠
    /// target tenant), and binding it would leak the other tenant's
    /// infrastructure config into this workspace's spawn decisions.
    #[tokio::test]
    async fn update_workspace_rejects_cross_tenant_compute_target() {
        let state = crate::mem::test_state();
        // Seed a compute target in a DIFFERENT tenant.
        let ct = gyre_domain::ComputeTargetEntity::new(
            gyre_common::Id::new("ct-other-1"),
            gyre_common::Id::new("other-tenant"),
            "evil-docker".to_string(),
            gyre_domain::ComputeTargetType::Container,
            100,
        );
        state.compute_targets.create(&ct).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "tenant_id": "t1", "name": "CtWs2", "slug": "ct-ws-2" });
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
        let id = created["id"].as_str().unwrap().to_string();

        let update = serde_json::json!({ "compute_target_id": "ct-other-1" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "cross-tenant compute target binding must be rejected"
        );

        // The workspace must be unchanged — no partial application.
        let ws_after = state
            .workspaces
            .find_by_id(&gyre_common::Id::new(&id))
            .await
            .unwrap()
            .expect("workspace still exists");
        assert!(
            ws_after.compute_target_id.is_none(),
            "rejected binding must not persist"
        );
    }

    /// A nonexistent compute target ID must be rejected with 400, not
    /// silently stored: a stale ID would fall through the spawn-time
    // fallback chain, hiding the misconfiguration.
    #[tokio::test]
    async fn update_workspace_rejects_unknown_compute_target() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let body = serde_json::json!({ "tenant_id": "t1", "name": "CtWs3", "slug": "ct-ws-3" });
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
        let id = created["id"].as_str().unwrap().to_string();

        let update = serde_json::json!({ "compute_target_id": "no-such-target" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unknown compute target must be rejected, not silently stored"
        );
    }

    /// TASK-077 (F7): Preset → Custom must PRESERVE the existing trust:
    /// policies (Custom means "operator manages policies manually").
    #[tokio::test]
    async fn trust_transition_preset_to_custom_preserves_trust_policies() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create with the entity-default Supervised level (seeds
        // trust:require-human-mr-review).
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

        // Sanity: the Supervised trust policy exists after create.
        let seeded = state.policies.list().await.unwrap();
        assert!(
            seeded
                .iter()
                .any(|p| p.name == "trust:require-human-mr-review"),
            "default Supervised create must seed trust:require-human-mr-review"
        );

        // Transition Supervised → Custom: trust policies must be preserved.
        let update = serde_json::json!({ "trust_level": "Custom" });
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
        assert_eq!(updated["trust_level"], "Custom");

        let policies = state.policies.list().await.unwrap();
        assert!(
            policies
                .iter()
                .any(|p| p.name == "trust:require-human-mr-review"),
            "Custom transition must PRESERVE existing trust: policies"
        );
    }

    /// TASK-077 (F7): Custom → preset must DELETE all trust: policies for the
    /// workspace and reseed the preset's policies. A non-trust user policy
    /// (no `trust:` prefix) must survive the transition.
    #[tokio::test]
    async fn trust_transition_custom_to_preset_deletes_and_reseeds() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Create Supervised (default) then transition to Custom.
        let body = serde_json::json!({ "tenant_id": "t1", "name": "W2", "slug": "w2" });
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

        let to_custom = serde_json::json!({ "trust_level": "Custom" });
        let custom_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&to_custom).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(custom_resp.status(), StatusCode::OK);

        // In Custom mode the operator adds their own policies — including
        // custom trust-prefixed ones and unrelated user policies.
        let base = gyre_domain::trust_policies_for_level(
            &gyre_domain::TrustLevel::Supervised,
            &ws_id,
            "operator",
        )
        .into_iter()
        .next()
        .unwrap();
        let custom_trust = gyre_domain::Policy {
            id: gyre_common::Id::new("trust-operator-custom-rule"),
            name: "trust:operator-custom-rule".to_string(),
            ..base.clone()
        };
        state.policies.create(&custom_trust).await.unwrap();

        let user_policy = gyre_domain::Policy {
            id: gyre_common::Id::new("user-no-trust-prefix"),
            name: "user:custom-policy".to_string(),
            ..base
        };
        state.policies.create(&user_policy).await.unwrap();

        // Transition Custom → Guided: all trust: policies for this workspace
        // must be deleted (including the operator's custom trust policy and
        // the seeded require-human-mr-review); the user policy survives and
        // Guided reseeds nothing.
        let to_guided = serde_json::json!({ "trust_level": "Guided" });
        let guided_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{ws_id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&to_guided).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(guided_resp.status(), StatusCode::OK);
        let updated = body_json(guided_resp).await;
        assert_eq!(updated["trust_level"], "Guided");

        let policies = state.policies.list().await.unwrap();
        assert!(
            !policies.iter().any(|p| p.name.starts_with("trust:")),
            "Custom → preset must DELETE all trust: policies for the workspace"
        );
        assert!(
            policies.iter().any(|p| p.name == "user:custom-policy"),
            "non-trust user policies must survive the Custom → preset transition"
        );
    }

    /// A misspelled trust_level in the update body must 400 — NOT silently
    /// coerce to the Supervised fallback. Without strict parsing, a typo
    /// like "Autonomus" on a Guided workspace returned 200 and rewrote the
    /// workspace + its trust: policies to Supervised while the caller
    /// believed they had selected Autonomous (a trust downgrade via typo —
    /// strictly more human review, but still an unintended policy rewrite
    /// the caller cannot distinguish from success).
    #[tokio::test]
    async fn update_workspace_invalid_trust_level_returns_400_and_leaves_state() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Guided workspace (explicit, so the assertions below pin a state
        // that differs from both the fallback and the typo's target).
        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w", "trust_level": "Guided" });
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

        let update = serde_json::json!({ "trust_level": "Autonomus" });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // State untouched: still Guided, no trust: policies seeded.
        let ws_after = state
            .workspaces
            .find_by_id(&gyre_common::Id::new(&ws_id))
            .await
            .unwrap()
            .expect("workspace still exists");
        assert_eq!(
            ws_after.trust_level,
            gyre_domain::TrustLevel::Guided,
            "rejected trust_level must not change the stored level"
        );
        let policies = state.policies.list().await.unwrap();
        assert!(
            !policies.iter().any(|p| p.name.starts_with("trust:")
                && p.scope_id.as_ref().map(|id| id.as_str()) == Some(ws_id.as_str())),
            "rejected trust_level must not seed trust: policies"
        );
    }

    /// Same contract on create: a typo'd trust_level must 400 instead of
    /// creating a Supervised workspace the caller cannot distinguish from
    /// the level they asked for.
    #[tokio::test]
    async fn create_workspace_invalid_trust_level_returns_400() {
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({ "tenant_id": "t1", "name": "W", "slug": "w", "trust_level": "autonomous" });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // No workspace row leaked through the rejected create.
        let workspaces = state
            .workspaces
            .list_by_tenant(&gyre_common::Id::new("t1"))
            .await
            .unwrap();
        assert!(
            workspaces.is_empty(),
            "rejected create must not persist a workspace"
        );
    }
}
