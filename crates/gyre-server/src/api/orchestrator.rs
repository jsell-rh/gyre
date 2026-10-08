//! Orchestrator Lifecycle Protocol (platform-model.md §3, task-093).
//!
//! Two-level orchestration: at most one live workspace orchestrator per
//! workspace, and at most one live repo orchestrator per repo. Orchestrators
//! get scope-bound JWTs (workspace_id [+ repo_id] + orchestrator_type) so the
//! MCP tier checks (see mcp.rs) can authorize cross-cutting tools. They carry
//! no task and no worktree - they coordinate, workers implement.
use super::budget;
use super::error::ApiError;
use super::spawn::{bootstrap_agent_keypair, orchestrator_response};
use super::{new_id, now_secs};
use crate::auth::AuthenticatedAgent;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use gyre_common::Id;
use gyre_domain::AnalyticsEvent;
use gyre_domain::{AgentStatus, MetaSpecApprovalStatus, MetaSpecKind, OrchestratorType};
use gyre_ports::MetaSpecFilter;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Response for orchestrator spawn: agent summary + scoped JWT. Orchestrators
/// have no worktree/branch/clone URL, so those fields are omitted entirely
/// rather than defaulted (task-093).
#[derive(Serialize)]
pub struct SpawnOrchestratorResponse {
    pub agent: super::spawn::OrchestratorAgentResponse,
    pub token: String,
}

#[derive(Deserialize)]
pub struct SpawnOrchestratorRequest {
    /// Free-form display name; defaults to a per-scope name.
    #[serde(default)]
    pub name: Option<String>,
    /// Parent agent (typically the workspace orchestrator for a repo
    /// orchestrator). Optional - human-initiated spawns have no parent.
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Whether an agent slot counts as occupying its scope. Dead/Stopped/Failed
/// orchestrators do NOT block a respawn - combined with auto-restart this
/// keeps exactly one live orchestrator per scope (§3.2).
fn is_live(a: &gyre_domain::Agent) -> bool {
    !matches!(
        a.status,
        AgentStatus::Dead | AgentStatus::Stopped | AgentStatus::Failed
    )
}

/// Soft persona validation (§3.1): warn and continue when the seeded persona
/// meta-spec is missing or unapproved - never block the spawn path on it.
async fn validate_persona(state: &AppState, name: &str) {
    let filter = MetaSpecFilter {
        kind: Some(MetaSpecKind::Persona),
        ..Default::default()
    };
    if let Ok(personas) = state.meta_specs.list(&filter).await {
        match personas.iter().find(|p| p.name == name) {
            None => tracing::warn!(persona = name, "persona meta-spec not found"),
            Some(p) if !matches!(p.approval_status, MetaSpecApprovalStatus::Approved) => {
                tracing::warn!(persona = name, "persona meta-spec not approved")
            }
            _ => {}
        }
    }
}

/// Common tail: persist agent, mint scoped JWT, register it, bootstrap the
/// signing keypair so the orchestrator can sign DerivedInputs for children,
/// bump budgets, track analytics.
#[allow(clippy::too_many_arguments)]
async fn spawn_orchestrator(
    state: &AppState,
    workspace_id: &Id,
    repo_id: Option<&Id>,
    orchestrator_type: OrchestratorType,
    name: String,
    parent_id: Option<String>,
    auth_agent_id: &str,
) -> Result<(gyre_domain::Agent, String), ApiError> {
    let now = now_secs();

    let mut agent = gyre_domain::Agent::new(new_id(), name, now);
    if let Ok(Some(_)) = state.agents.find_by_name(&agent.name).await {
        return Err(ApiError::InvalidInput(format!(
            "an agent named '{}' already exists; choose a different name",
            agent.name
        )));
    }

    agent.parent_id = parent_id.map(Id::new);
    agent.spawned_by = Some(auth_agent_id.to_string());
    agent.workspace_id = workspace_id.clone();
    agent.repo_id = repo_id.cloned();
    agent.orchestrator_type = orchestrator_type.clone();
    agent.restart_on_failure = true;
    // Exactly-one-live semantics (§3.2): orchestrators abort on heartbeat
    // timeout so the stale detector restarts a replacement and (repo tier)
    // escalates the death to the workspace orchestrator.
    agent.disconnected_behavior = gyre_domain::DisconnectedBehavior::Abort;
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.create(&agent).await?;

    // Scoped JWT: workspace tier carries workspace_id only; repo tier carries
    // workspace_id + repo_id. task_id carries the orchestrator's own id
    // (orchestrators are not task-bound but the claim is required on all
    // agent JWTs for gyre_agent_complete compatibility).
    let token = state
        .agent_signing_key
        .mint_orchestrator(
            &agent.id.to_string(),
            auth_agent_id,
            &state.base_url,
            state.agent_jwt_ttl_secs,
            &workspace_id.to_string(),
            repo_id.map(|r| r.to_string()).as_deref(),
            &orchestrator_type.to_string(),
        )
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("orchestrator token mint: {e}")))?;
    let _ = state
        .kv_store
        .kv_set("agent_tokens", &agent.id.to_string(), token.clone())
        .await;

    // Keypair so the orchestrator can sign DerivedInputs when spawning
    // children (authorization-provenance.md §4.5).
    bootstrap_agent_keypair(state, &agent.id.to_string(), now).await;

    // Auto-track spawn (analytics.md §Auto-Emitted Events).
    let persona_name = match orchestrator_type {
        OrchestratorType::Worker => "default-worker",
        OrchestratorType::WorkspaceOrchestrator => "workspace-orchestrator",
        OrchestratorType::RepoOrchestrator => "repo-orchestrator",
    };
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({
            "task_id": agent.id.to_string(),
            "compute_target": "local",
            "persona": persona_name,
            "orchestrator_type": orchestrator_type.to_string(),
            "workspace_id": workspace_id.to_string(),
            "repo_id": repo_id.map(|r| r.to_string()),
        }),
        now,
    )
    .with_scope(None, None, Some(workspace_id), repo_id);
    let _ = state.analytics.record(&ev).await;

    budget::increment_active_agents(state, &workspace_id.to_string()).await;

    Ok((agent, token))
}

/// Shared core of POST /api/v1/workspaces/:id/orchestrator/spawn (task-093).
pub(crate) async fn spawn_workspace_orchestrator_core(
    state: &AppState,
    workspace_id: &str,
    req: SpawnOrchestratorRequest,
    auth_agent_id: &str,
) -> Result<(gyre_domain::Agent, String), ApiError> {
    let ws_id = Id::new(workspace_id.to_string());
    let workspace = match state.workspaces.find_by_id(&ws_id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            return Err(ApiError::NotFound(format!(
                "workspace {workspace_id} not found"
            )))
        }
        Err(e) => return Err(ApiError::Internal(e)),
    };

    // One live workspace orchestrator per workspace (§3.2).
    let existing = state.agents.list_by_workspace(&ws_id).await?;
    if existing
        .iter()
        .any(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator && is_live(a))
    {
        return Err(ApiError::Conflict(
            "a workspace orchestrator is already active for this workspace".to_string(),
        ));
    }

    budget::check_spawn_budget(state, &workspace.id.to_string())
        .await
        .map_err(ApiError::TooManyRequests)?;

    validate_persona(state, "workspace-orchestrator").await;

    let name = req
        .name
        .unwrap_or_else(|| format!("workspace-orchestrator-{}", workspace.id));
    spawn_orchestrator(
        state,
        &workspace.id,
        None,
        OrchestratorType::WorkspaceOrchestrator,
        name,
        req.parent_id,
        auth_agent_id,
    )
    .await
}

/// POST /api/v1/workspaces/:id/orchestrator/spawn
///
/// Spawn the single workspace orchestrator for a workspace.
/// Returns 409 Conflict when a live workspace orchestrator already exists.
pub async fn spawn_workspace_orchestrator(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Json(req): Json<SpawnOrchestratorRequest>,
) -> Result<(StatusCode, Json<SpawnOrchestratorResponse>), ApiError> {
    let (agent, token) =
        spawn_workspace_orchestrator_core(&state, &workspace_id, req, &auth.agent_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(SpawnOrchestratorResponse {
            agent: orchestrator_response(agent),
            token,
        }),
    ))
}

/// List live repo orchestrators in a workspace (§3.1: spawn/restart repo
/// orchestrators is a workspace-orchestrator responsibility). Shared by the
/// MCP tool `gyre_list_repo_orchestrators`.
pub(crate) async fn list_repo_orchestrators_core(
    state: &AppState,
    workspace_id: &str,
) -> Result<Vec<gyre_domain::Agent>, ApiError> {
    let ws_id = Id::new(workspace_id.to_string());
    if state.workspaces.find_by_id(&ws_id).await?.is_none() {
        return Err(ApiError::NotFound(format!(
            "workspace {workspace_id} not found"
        )));
    }
    let all = state.agents.list_by_workspace(&ws_id).await?;
    Ok(all
        .into_iter()
        .filter(|a| a.orchestrator_type == OrchestratorType::RepoOrchestrator && is_live(a))
        .collect())
}

/// Shared core of POST /api/v1/repos/:id/orchestrator/spawn (task-093).
///
/// Used by both the REST handler and the MCP tool `gyre_spawn_repo_orchestrator`
/// so the two surfaces cannot diverge (check-mcp-wrapper-parity).
/// Caller must already be authorized for the target repo (REST: ABAC check;
/// MCP: workspace-tier JWT + same-workspace repo check).
pub(crate) async fn spawn_repo_orchestrator_core(
    state: &AppState,
    repo_id: &str,
    req: SpawnOrchestratorRequest,
    auth_agent_id: &str,
) -> Result<(gyre_domain::Agent, String), ApiError> {
    let rid = Id::new(repo_id.to_string());
    let repo = state
        .repos
        .find_by_id(&rid)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let ws_id = repo.workspace_id.clone();

    // One live repo orchestrator per repo (§3.2).
    let existing = state.agents.list_by_workspace(&ws_id).await?;
    if existing.iter().any(|a| {
        a.orchestrator_type == OrchestratorType::RepoOrchestrator
            && a.repo_id.as_ref() == Some(&rid)
            && is_live(a)
    }) {
        return Err(ApiError::Conflict(format!(
            "a repo orchestrator is already active for repo {repo_id}"
        )));
    }

    budget::check_spawn_budget(state, &ws_id.to_string())
        .await
        .map_err(ApiError::TooManyRequests)?;

    validate_persona(state, "repo-orchestrator").await;

    let name = req
        .name
        .unwrap_or_else(|| format!("repo-orchestrator-{}", repo.id));
    spawn_orchestrator(
        state,
        &ws_id,
        Some(&rid),
        OrchestratorType::RepoOrchestrator,
        name,
        req.parent_id,
        auth_agent_id,
    )
    .await
}

/// POST /api/v1/repos/:id/orchestrator/spawn
///
/// Spawn the single repo orchestrator for a repo. Returns 409 Conflict when
/// a live repo orchestrator already exists for that repo.
pub async fn spawn_repo_orchestrator(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    Json(req): Json<SpawnOrchestratorRequest>,
) -> Result<(StatusCode, Json<SpawnOrchestratorResponse>), ApiError> {
    // G6: ABAC enforcement - repo access required to spawn its orchestrator.
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;

    let (agent, token) =
        spawn_repo_orchestrator_core(&state, &repo_id, req, &auth.agent_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(SpawnOrchestratorResponse {
            agent: orchestrator_response(agent),
            token,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    fn req(name: Option<&str>) -> SpawnOrchestratorRequest {
        SpawnOrchestratorRequest {
            name: name.map(|n| n.to_string()),
            parent_id: None,
        }
    }

    /// Seed ws-1 with tenant t1 plus two repos r-1, r-2.
    async fn seed(state: &crate::AppState) {
        let ws = gyre_domain::Workspace::new(Id::new("ws-1"), Id::new("t1"), "Ws", "ws", 0);
        state.workspaces.create(&ws).await.unwrap(); // non-atomic-create:ok — test seed, in-memory test state
        for rid in ["r-1", "r-2"] {
            let repo = gyre_domain::Repository::new(
                Id::new(rid),
                Id::new("ws-1"),
                rid,
                format!("/tmp/{rid}"),
                0,
            );
            state.repos.create(&repo).await.unwrap(); // non-atomic-create:ok — test seed, in-memory test state
        }
    }

    /// Mark a live agent Dead in the store (frees its scope slot, §3.2).
    async fn kill(state: &crate::AppState, agent_name: &str) {
        let mut a = state
            .agents
            .find_by_name(agent_name)
            .await
            .unwrap()
            .expect("agent exists");
        a.transition_status(AgentStatus::Dead).unwrap();
        state.agents.update(&a).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_orchestrator_spawn_and_scoped_jwt() {
        let state = test_state();
        seed(&state).await;

        let (agent, token) = spawn_workspace_orchestrator_core(&state, "ws-1", req(None), "user-1")
            .await
            .unwrap();

        assert_eq!(
            agent.orchestrator_type,
            OrchestratorType::WorkspaceOrchestrator
        );
        assert_eq!(agent.workspace_id, Id::new("ws-1"));
        assert!(agent.repo_id.is_none());
        assert!(agent.restart_on_failure);
        assert_eq!(agent.status, AgentStatus::Active);
        assert_eq!(agent.spawned_by.as_deref(), Some("user-1"));

        // Scoped JWT: workspace tier carries workspace_id, no repo_id (§3.1).
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .unwrap();
        assert_eq!(claims.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(claims.repo_id, None);
        assert_eq!(
            claims.orchestrator_type.as_deref(),
            Some("workspace_orchestrator")
        );
        assert_eq!(claims.scope, "agent");

        // Token registered so the auth middleware accepts it.
        let stored = state
            .kv_store
            .kv_get("agent_tokens", &agent.id.to_string())
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some(token.as_str()));
    }

    #[tokio::test]
    async fn second_workspace_orchestrator_conflicts_409() {
        let state = test_state();
        seed(&state).await;

        spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch-a")), "user-1")
            .await
            .unwrap();
        let err =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch-b")), "user-1")
                .await
                .unwrap_err();
        assert!(matches!(err, ApiError::Conflict(_)), "got: {err}");

        // Dead orchestrator frees the slot (§3.2 exactly-one-live).
        kill(&state, "ws-orch-a").await;
        spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch-b")), "user-1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn repo_orchestrator_spawn_scoped_jwt_and_409() {
        let state = test_state();
        seed(&state).await;

        let (agent, token) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("repo-orch-1")), "user-1")
                .await
                .unwrap();
        assert_eq!(agent.orchestrator_type, OrchestratorType::RepoOrchestrator);
        assert_eq!(agent.repo_id, Some(Id::new("r-1")));

        // Scoped JWT: repo tier carries workspace_id + repo_id (§3.1).
        let claims = state
            .agent_signing_key
            .validate(&token, &state.base_url)
            .unwrap();
        assert_eq!(claims.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(claims.repo_id.as_deref(), Some("r-1"));
        assert_eq!(
            claims.orchestrator_type.as_deref(),
            Some("repo_orchestrator")
        );

        // One per repo, but a different repo in the same workspace is fine.
        let err = spawn_repo_orchestrator_core(&state, "r-1", req(Some("repo-orch-2")), "user-1")
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::Conflict(_)), "got: {err}");
        spawn_repo_orchestrator_core(&state, "r-2", req(Some("repo-orch-2")), "user-1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn repo_orchestrator_response_has_no_duplicate_repo_id() {
        // Regression: orchestrator_response flattened AgentResponse.repo_id
        // alongside the outer repo_id, emitting the JSON key twice — strict
        // deserializers (serde default) reject duplicate fields, so the CLI
        // could not parse the spawn response at all.
        let state = test_state();
        seed(&state).await;
        let (agent, _token) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("dup-check")), "user-1")
                .await
                .unwrap();
        let json = serde_json::to_value(orchestrator_response(agent)).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(
            obj.get("repo_id"),
            Some(&serde_json::json!("r-1")),
            "outer repo_id must carry the orchestrator's repo binding"
        );
        let count = obj.keys().filter(|k| *k == "repo_id").count();
        assert_eq!(count, 1, "repo_id must appear exactly once: {obj:?}");
        assert_eq!(obj["orchestrator_type"], "repo_orchestrator");
        assert_eq!(obj["restart_on_failure"], true);
    }

    #[tokio::test]
    async fn stale_detector_restarts_dead_orchestrator() {
        let state = test_state();
        seed(&state).await;

        let (agent, _token) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch")), "user-1")
                .await
                .unwrap();

        // Age the agent past the heartbeat timeout, then run one cycle.
        let mut aged = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
        aged.last_heartbeat = None;
        state.agents.update(&aged).await.unwrap();

        crate::stale_agents::run_once(&state).await.unwrap();

        // Old agent Dead, replacement live with same scope + restart suffix.
        let dead = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        assert_eq!(dead.status, AgentStatus::Dead);
        let peers = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        let replacement = peers
            .iter()
            .find(|a| a.name == "ws-orch-restart-1")
            .expect("replacement spawned");
        assert_eq!(replacement.status, AgentStatus::Active);
        assert_eq!(
            replacement.orchestrator_type,
            OrchestratorType::WorkspaceOrchestrator
        );
        assert_eq!(replacement.workspace_id, Id::new("ws-1"));

        // Replacement got a scoped JWT too.
        let tok = state
            .kv_store
            .kv_get("agent_tokens", &replacement.id.to_string())
            .await
            .unwrap()
            .expect("replacement token");
        let claims = state
            .agent_signing_key
            .validate(&tok, &state.base_url)
            .unwrap();
        assert_eq!(
            claims.orchestrator_type.as_deref(),
            Some("workspace_orchestrator")
        );
    }

    #[tokio::test]
    async fn repo_orchestrator_death_escalates_to_workspace_orchestrator() {
        let state = test_state();
        seed(&state).await;

        let (ws_orch, _t1) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch")), "user-1")
                .await
                .unwrap();
        let (repo_orch, _t2) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("repo-orch")), "user-1")
                .await
                .unwrap();

        // Age the repo orchestrator past the heartbeat timeout and run a cycle.
        let mut aged = state
            .agents
            .find_by_id(&repo_orch.id)
            .await
            .unwrap()
            .unwrap();
        aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
        aged.last_heartbeat = None;
        state.agents.update(&aged).await.unwrap();

        crate::stale_agents::run_once(&state).await.unwrap();

        // The workspace orchestrator received an Escalation message.
        let inbox = state.messages.list_unacked(&ws_orch.id, 100).await.unwrap();
        assert!(
            inbox
                .iter()
                .any(|m| m.kind == gyre_common::message::MessageKind::Escalation),
            "expected an Escalation message in the workspace orchestrator inbox"
        );
    }
}
