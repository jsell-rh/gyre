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
use gyre_domain::{AgentStatus, OrchestratorType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Response for orchestrator spawn: agent summary + scoped JWT. Orchestrators
/// have no worktree/branch/clone URL, so those fields are omitted entirely
/// rather than defaulted (task-093).
pub struct SpawnOrchestratorResponse {
    pub agent: super::spawn::OrchestratorAgentResponse,
    pub token: String,
    /// F3: truthful process status -- "running" or "launch_failed". A
    /// persisted agent row alone is not a running orchestrator.
    pub launch_status: String,
    /// Failure reason when launch_status == "launch_failed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_detail: Option<String>,
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

/// Resolve the persona an orchestrator spawn must attach (platform-model.md
/// §3: "spawn repo orchestrator agent with repo-orchestrator persona").
///
/// Consults `state.personas` (the store `gyre bootstrap` step 5 populates),
/// NOT `state.meta_specs`. Resolution is nearest-scope-wins (§2):
/// Repo > Workspace > Tenant. Fail-closed: a persona that does not resolve,
/// or resolves but is not Approved, rejects the spawn — an orchestrator
/// running without its persona is an unstyled coordination agent, which is
/// exactly what the spec forbids.
async fn resolve_orchestrator_persona(
    state: &AppState,
    slug: &str,
    workspace_id: &Id,
    repo_id: Option<&Id>,
) -> Result<gyre_domain::Persona, ApiError> {
    use gyre_domain::{PersonaApprovalStatus, PersonaScope};

    // Tenant scope id comes from the workspace record (the hierarchy root
    // of this spawn) — never fabricated from a literal.
    let tenant_id = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?
        .tenant_id;

    let mut scopes: Vec<PersonaScope> = Vec::new();
    if let Some(rid) = repo_id {
        scopes.push(PersonaScope::Repo(rid.clone()));
    }
    scopes.push(PersonaScope::Workspace(workspace_id.clone()));
    scopes.push(PersonaScope::Tenant(tenant_id));

    for scope in &scopes {
        if let Some(persona) = state
            .personas
            .find_by_slug_and_scope(slug, scope)
            .await
            .map_err(ApiError::Internal)?
        {
            if persona.approval_status != PersonaApprovalStatus::Approved {
                return Err(ApiError::Conflict(format!(
                    "persona '{slug}' resolves in scope {:?} but is {:?}; \
                     approve it before spawning its orchestrator",
                    scope, persona.approval_status
                )));
            }
            return Ok(persona);
        }
    }

    Err(ApiError::NotFound(format!(
        "persona '{slug}' not found in scope chain (repo/workspace/tenant); \
         register and approve it first (gyre bootstrap step 5 does this)"
    )))
}

/// Launch outcome for an orchestrator process (F3: a persisted row is not
/// "running" -- the spawn response must report what actually happened).
#[derive(Clone)]
pub(crate) struct LaunchOutcome {
    /// "running" or "launch_failed" (mirrors spawn.rs best-effort launch).
    pub launch_status: String,
    /// Failure reason when launch_status == "launch_failed".
    pub launch_detail: Option<String>,
}

/// Launch the orchestrator process on the workspace's compute target (§3:
/// both orchestrator tiers use the workspace's configured compute target).
///
/// Mirrors the agent spawn path (api/spawn.rs): compute-target priority
/// request → workspace assignment → tenant default → local. The command is
/// server-controlled only (GYRE_ORCHESTRATOR_COMMAND env or the agent image
/// entrypoint) -- never user input (C-1 RCE fix).
///
/// Failure is reported, not swallowed: the caller surfaces it in the spawn
/// response so `gyre bootstrap` prints a truthful orchestrator status. A
/// launch-failed orchestrator keeps restart_on_failure=true; the stale
/// detector replaces it once its heartbeat times out.
pub(crate) async fn launch_orchestrator_process(
    state: &AppState,
    agent: &gyre_domain::Agent,
    workspace: &gyre_domain::Workspace,
    token: &str,
) -> LaunchOutcome {
    // Compute-target priority: workspace assignment -> tenant default -> local.
    let target_config: Option<super::compute::ComputeTargetConfig> = workspace
        .compute_target_id
        .as_ref()
        .and_then(|ct_id| state.compute_targets.get_by_id(ct_id).await.ok().flatten())
        .or_else(|| {
            // Tenant default resolved synchronously is not possible here;
            // handled below via get_default_for_tenant.
            None
        })
        .map(|e| super::compute::ComputeTargetConfig {
            id: e.id.to_string(),
            name: e.name.clone(),
            target_type: match e.target_type {
                gyre_domain::ComputeTargetType::Container => "container".to_string(),
                gyre_domain::ComputeTargetType::Ssh => "ssh".to_string(),
                gyre_domain::ComputeTargetType::Kubernetes => "kubernetes".to_string(),
            },
            config: e.config.clone(),
        });
    // Tenant default when the workspace has no assignment.
    let target_config = match target_config {
        Some(cfg) => Some(cfg),
        None => state
            .compute_targets
            .get_default_for_tenant(&workspace.tenant_id)
            .await
            .ok()
            .flatten()
            .map(|e| super::compute::ComputeTargetConfig {
                id: e.id.to_string(),
                name: e.name.clone(),
                target_type: match e.target_type {
                    gyre_domain::ComputeTargetType::Container => "container".to_string(),
                    gyre_domain::ComputeTargetType::Ssh => "ssh".to_string(),
                    gyre_domain::ComputeTargetType::Kubernetes => "kubernetes".to_string(),
                },
                config: e.config.clone(),
            }),
    };

    // Server-controlled command: compute-target config, GYRE_ORCHESTRATOR_COMMAND,
    // or the agent image entrypoint (same contract as api/spawn.rs).
    let command = target_config
        .as_ref()
        .and_then(|cfg| cfg.config.get("command"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("GYRE_ORCHESTRATOR_COMMAND").ok())
        .or_else(|| std::env::var("GYRE_AGENT_COMMAND").ok())
        .unwrap_or_else(|| "/gyre/entrypoint.sh".to_string());
    let args: Vec<String> = target_config
        .as_ref()
        .and_then(|cfg| cfg.config.get("args"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Agent context env vars (agent-protocol.md M24). The orchestrator's
    // "task" claim is its own id (orchestrators are not task-bound).
    let mut env = std::collections::HashMap::new();
    env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
    env.insert("GYRE_AUTH_TOKEN".to_string(), token.to_string());
    env.insert("GYRE_AGENT_ID".to_string(), agent.id.to_string());
    env.insert("GYRE_TASK_ID".to_string(), agent.id.to_string());
    env.insert("GYRE_ORCHESTRATOR".to_string(), agent.orchestrator_type.to_string());
    if let Some(rid) = &agent.repo_id {
        env.insert("GYRE_REPO_ID".to_string(), rid.to_string());
    }
    env.insert("GYRE_WORKSPACE_ID".to_string(), agent.workspace_id.to_string());

    let spawn_config = gyre_ports::SpawnConfig {
        name: agent.name.clone(),
        command: command.clone(),
        args: args.clone(),
        env,
        // Local fallback work dir: absolute /tmp (the orchestrator clones
        // from GYRE_SERVER_URL; no worktree exists for it).
        work_dir: "/tmp".to_string(),
    };

    let launch_result = match &target_config {
        Some(cfg) if cfg.target_type == "container" => {
            let image = cfg.config["image"].as_str().unwrap_or("gyre-agent:latest").to_string();
            let mut ct = gyre_adapters::compute::ContainerTarget::new(image.clone());
            ct = ct.with_network(cfg.config["network"].as_str().unwrap_or("none"));
            if let Some(mem) = cfg.config["memory_limit"].as_str() {
                ct = ct.with_memory_limit(mem);
            }
            if let Some(pids) = cfg.config["pids_limit"].as_u64() {
                ct = ct.with_pids_limit(pids as u32);
            }
            gyre_ports::ComputeTarget::spawn_process(&ct, &spawn_config).await
        }
        _ => {
            // Default: local process spawn.
            let local = gyre_adapters::compute::LocalTarget;
            gyre_ports::ComputeTarget::spawn_process(&local, &spawn_config).await
        }
    };

    match launch_result {
        Ok(handle) => {
            let agent_id_str = agent.id.to_string();
            state
                .process_registry
                .lock()
                .await
                .insert(agent_id_str.clone(), handle.clone());
            // Monitor: on exit, free the registry slot and drop the agent to
            // Idle so the stale detector / restart loop takes over.
            let state_mon = std::sync::Arc::clone(state);
            let orch_type = agent.orchestrator_type.clone();
            tokio::spawn(async move {
                let local = gyre_adapters::compute::LocalTarget;
                let _ = &local;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    let alive =
                        gyre_ports::ComputeTarget::is_alive(&local, &handle).await.unwrap_or(false);
                    if !alive {
                        state_mon
                            .process_registry
                            .lock()
                            .await
                            .remove(&agent_id_str);
                        if let Ok(Some(mut a)) =
                            state_mon.agents.find_by_id(&Id::new(&agent_id_str)).await
                        {
                            if a.status == AgentStatus::Active && orch_type == a.orchestrator_type
                            {
                                // Orchestrator process died; mark Dead so the
                                // stale detector spawns a replacement.
                                let _ = a.transition_status(AgentStatus::Dead);
                                let _ = state_mon.agents.update(&a).await;
                            }
                        }
                        break;
                    }
                }
            });
            LaunchOutcome {
                launch_status: "running".to_string(),
                launch_detail: None,
            }
        }
        Err(e) => LaunchOutcome {
            launch_status: "launch_failed".to_string(),
            launch_detail: Some(format!("{e}")),
        },
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
    persona_id: Id,
) -> Result<(gyre_domain::Agent, String, LaunchOutcome), ApiError> {
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
    // F2: persona binding persisted on the agent row (§3 step 8).
    agent.persona_id = Some(persona_id);
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

    // Auto-track spawn.
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({
            "orchestrator_type": orchestrator_type.to_string(),
            "workspace_id": workspace_id.to_string(),
            "repo_id": repo_id.map(|r| r.to_string()),
        }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    budget::increment_active_agents(state, &workspace_id.to_string()).await;

    // F3: launch the orchestrator process (workspace compute target). The
    // outcome is returned so the REST/MCP responses report a truthful
    // status -- a persisted row alone is not "running".
    let workspace = state
        .workspaces
        .find_by_id(workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    let launch = launch_orchestrator_process(state, &agent, &workspace, &token).await;

    Ok((agent, token, launch))
}

/// Shared core of POST /api/v1/workspaces/:id/orchestrator/spawn (task-093).
pub(crate) async fn spawn_workspace_orchestrator_core(
    state: &AppState,
    workspace_id: &str,
    req: SpawnOrchestratorRequest,
    auth_agent_id: &str,
) -> Result<(gyre_domain::Agent, String, LaunchOutcome), ApiError> {
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

    let persona = resolve_orchestrator_persona(
        state,
        "workspace-orchestrator",
        &ws_id,
        None,
    )
    .await?;

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
        persona.id,
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
    let (agent, token, launch) =
        spawn_workspace_orchestrator_core(&state, &workspace_id, req, &auth.agent_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(SpawnOrchestratorResponse {
            agent: orchestrator_response(agent),
            token,
            launch_status: launch.launch_status,
            launch_detail: launch.launch_detail,
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
) -> Result<(gyre_domain::Agent, String, LaunchOutcome), ApiError> {
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

    let persona = resolve_orchestrator_persona(state, "repo-orchestrator", &ws_id, Some(&rid)).await?;

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
        persona.id,
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

    let (agent, token, launch) =
        spawn_repo_orchestrator_core(&state, &repo_id, req, &auth.agent_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(SpawnOrchestratorResponse {
            agent: orchestrator_response(agent),
            token,
            launch_status: launch.launch_status,
            launch_detail: launch.launch_detail,
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

    /// Seed ws-1 with tenant t1 plus two repos r-1, r-2, and the two
    /// orchestrator personas pre-approved at tenant scope (bootstrap step 5
    /// registers exactly these; F2 makes spawn require them).
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
        for slug in ["workspace-orchestrator", "repo-orchestrator"] {
            let mut persona = gyre_domain::Persona::new(
                Id::new(format!("{slug}-persona")),
                slug,
                slug,
                gyre_domain::PersonaScope::Tenant(Id::new("t1")),
                "test orchestrator persona",
                0,
            );
            persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
            state.personas.create(&persona).await.unwrap(); // non-atomic-create:ok — test seed, in-memory test state
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
        // F2: persona resolved and attached (workspace-orchestrator persona).
        assert_eq!(
            agent.persona_id.as_ref().map(|p| p.to_string()),
            Some("workspace-orchestrator-persona".to_string())
        );

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
        // F2: persona resolved and attached (repo-orchestrator persona).
        assert_eq!(
            agent.persona_id.as_ref().map(|p| p.to_string()),
            Some("repo-orchestrator-persona".to_string())
        );

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
