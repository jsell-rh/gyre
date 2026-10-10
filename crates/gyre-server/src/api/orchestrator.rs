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
#[derive(Serialize)]
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
#[derive(Clone, Debug)]
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
    state: &Arc<AppState>,
    agent: &gyre_domain::Agent,
    workspace: &gyre_domain::Workspace,
    token: &str,
) -> LaunchOutcome {
    // Compute-target priority: workspace assignment -> tenant default -> local.
    // (The workspace lookup needs .await, which cannot sit inside the sync
    // and_then closure, so resolve it at statement level first.)
    let workspace_target = match workspace.compute_target_id.as_ref() {
        Some(ct_id) => state.compute_targets.get_by_id(ct_id).await.ok().flatten(),
        None => None,
    };
    let target_config: Option<super::compute::ComputeTargetConfig> = workspace_target
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

    // Probe info for the exit monitor: it must probe liveness with the same
    // target type the process launched on (a container handle has no pid, so
    // a LocalTarget probe would report dead instantly). SSH liveness probes
    // re-open a connection, so the monitor keeps the connection parameters.
    let mut launch_image: Option<String> = None;
    let mut launch_ssh: Option<(String, String, Option<String>, Option<u16>)> = None;

    // Launch on the resolved target. Mirrors api/spawn.rs: container via
    // ContainerTarget (G8 security defaults), ssh via SshTarget (with
    // optional remote container_mode), everything else (local, kubernetes
    // until a k8s adapter exists) via LocalTarget.
    let launch_result = match &target_config {
        Some(cfg) if cfg.target_type == "container" => {
            let image = cfg.config["image"].as_str().unwrap_or("gyre-agent:latest").to_string();
            launch_image = Some(image.clone());
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
        Some(cfg) if cfg.target_type == "ssh" => {
            // M19.5 SSH remote spawn (same shape as api/spawn.rs).
            let user = cfg.config["user"].as_str().unwrap_or("root").to_string();
            let host = cfg.config["host"].as_str().unwrap_or("localhost").to_string();
            launch_ssh = Some((
                user.clone(),
                host.clone(),
                cfg.config["identity_file"].as_str().map(str::to_string),
                cfg.config["port"].as_u64().map(|p| p as u16),
            ));
            let mut ssh_target = gyre_adapters::compute::SshTarget::new(user, host);
            if let Some(id_file) = cfg.config["identity_file"].as_str() {
                ssh_target = ssh_target.with_identity(id_file);
            }
            if let Some(port) = cfg.config["port"].as_u64() {
                ssh_target = ssh_target.with_port(port as u16);
            }
            let container_mode = cfg.config["container_mode"].as_bool().unwrap_or(false);
            let ssh_spawn_config = if container_mode {
                // Wrap the command in a docker run on the remote host.
                // Validate the agent name to prevent injection (M19.5-A) —
                // direct args, no shell.
                let safe_name = agent
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
                if !safe_name || agent.name.is_empty() || agent.name.len() > 63 {
                    return LaunchOutcome {
                        launch_status: "launch_failed".to_string(),
                        launch_detail: Some(
                            "agent name must be 1-63 chars of [a-zA-Z0-9._-] \
                             for remote container use"
                                .to_string(),
                        ),
                    };
                }
                let image = cfg.config["image"].as_str().unwrap_or("gyre-agent:latest");
                let mut docker_args = vec![
                    "run".to_string(),
                    "--detach".to_string(),
                    "--rm".to_string(),
                    "--network=none".to_string(),
                    "--memory=2g".to_string(),
                    "--pids-limit=512".to_string(),
                    "--user=65534:65534".to_string(),
                    format!("--name={}", agent.name),
                    image.to_string(),
                    command.clone(),
                ];
                docker_args.extend(args.iter().cloned());
                gyre_ports::SpawnConfig {
                    name: agent.name.clone(),
                    command: "docker".to_string(),
                    args: docker_args,
                    env: std::collections::HashMap::new(),
                    work_dir: spawn_config.work_dir.clone(),
                }
            } else {
                spawn_config
            };
            gyre_ports::ComputeTarget::spawn_process(&ssh_target, &ssh_spawn_config).await
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
            // Monitor: on exit, free the registry slot and mark the agent
            // Dead so the stale detector / restart loop takes over. The
            // is_alive probe MUST use the same target type the process was
            // launched with — a container handle carries no pid, so probing
            // it with LocalTarget::is_alive reports dead instantly and would
            // kill a healthy orchestrator 5s after launch.
            let state_mon = std::sync::Arc::clone(state);
            let orch_type = agent.orchestrator_type.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    let alive = match handle.target_type.as_str() {
                        "container" => {
                            // Rebuild the probe target from the launch
                            // config stored on the registry handle.
                            let image = launch_image
                                .as_deref()
                                .unwrap_or("gyre-agent:latest")
                                .to_string();
                            let ct = gyre_adapters::compute::ContainerTarget::new(image);
                            gyre_ports::ComputeTarget::is_alive(&ct, &handle).await
                        }
                        "ssh" => {
                            // SSH liveness is probed by remote pid. When the
                            // probe itself cannot run (connection refused,
                            // ssh missing), do NOT declare the orchestrator
                            // dead — the stale detector still owns
                            // heartbeat-based recovery.
                            match &launch_ssh {
                                Some((user, host, id_file, port)) => {
                                    let mut probe =
                                        gyre_adapters::compute::SshTarget::new(user.clone(), host.clone());
                                    if let Some(f) = id_file {
                                        probe = probe.with_identity(f.clone());
                                    }
                                    if let Some(p) = port {
                                        probe = probe.with_port(*p);
                                    }
                                    gyre_ports::ComputeTarget::is_alive(&probe, &handle).await
                                }
                                None => Ok(true),
                            }
                        }
                        _ => {
                            let local = gyre_adapters::compute::LocalTarget;
                            gyre_ports::ComputeTarget::is_alive(&local, &handle).await
                        }
                    }
                    .unwrap_or(true);
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
    state: &Arc<AppState>,
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
    state: &Arc<AppState>,
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
///
/// Per-handler authorization: the workspace's tenant must match the
/// caller's tenant (the route runs without middleware ABAC, so this is the
/// only cross-tenant containment). The global system token (agent_id
/// "system") bypasses — its tenant is the bootstrap principal, not a
/// tenant scope (see auth.rs `system_principal_tenant`).
pub async fn spawn_workspace_orchestrator(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Json(req): Json<SpawnOrchestratorRequest>,
) -> Result<(StatusCode, Json<SpawnOrchestratorResponse>), ApiError> {
    // Tenant containment: load the workspace, compare its tenant against
    // the caller's resolved tenant.
    let ws_id = Id::new(workspace_id.clone());
    let workspace = state
        .workspaces
        .find_by_id(&ws_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if auth.agent_id != "system" && auth.tenant_id != workspace.tenant_id.to_string() {
        return Err(ApiError::Forbidden(
            "workspace does not belong to the caller's tenant".to_string(),
        ));
    }

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
    state: &Arc<AppState>,
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
///
/// Authorization: JWT bearers are evaluated against the repo's ABAC policy
/// (G6), AND every caller is tenant-contained — the repo's workspace tenant
/// must match the caller's resolved tenant. The ABAC layer alone is not
/// containment: it returns Ok when no per-repo policies are stored and
/// bypasses for global tokens / API keys. The global system token
/// (agent_id "system") bypasses the tenant comparison — its tenant is the
/// bootstrap principal, not a tenant scope.
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

    // Tenant containment: load the repo, resolve its workspace, compare the
    // workspace's tenant against the caller's.
    let rid = Id::new(repo_id.clone());
    let repo = state
        .repos
        .find_by_id(&rid)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    let workspace = state.workspaces.find_by_id(&repo.workspace_id).await?;
    if let Some(ws) = workspace {
        if auth.agent_id != "system" && auth.tenant_id != ws.tenant_id.to_string() {
            return Err(ApiError::Forbidden(
                "repo does not belong to the caller's tenant".to_string(),
            ));
        }
    }

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

        let (agent, token, _launch) = spawn_workspace_orchestrator_core(&state, "ws-1", req(None), "user-1")
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

        let (agent, token, _launch) =
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
        let (agent, _token, _launch) =
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

        let (agent, _token, _launch) =
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

        let (ws_orch, _t1, _l1) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(Some("ws-orch")), "user-1")
                .await
                .unwrap();
        let (repo_orch, _t2, _l2) =
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

    #[tokio::test]
    async fn spawn_reports_truthful_launch_outcome() {
        // task-099 F3: the spawn outcome must report what actually happened
        // at launch -- never a blanket "running" for a row whose process
        // never started. Both outcomes are pinned deterministically via a
        // workspace compute target whose configured command either runs
        // (/bin/true) or does not exist.
        let state = test_state();
        seed(&state).await;

        let ok = launch_with_command(&state, "ct-ok", "/bin/true").await;
        assert_eq!(ok.launch_status, "running");
        assert!(ok.launch_detail.is_none());

        // The original F3 defect reported "running" unconditionally; a
        // nonexistent command must report launch_failed with a reason.
        let bad = launch_with_command(&state, "ct-bad", "/no/such/entrypoint").await;
        assert_eq!(bad.launch_status, "launch_failed");
        let detail = bad
            .launch_detail
            .as_deref()
            .expect("launch_failed must carry a reason");
        assert!(!detail.is_empty());

        // The REST response serializes the same fields (what the CLI parses).
        let agent = state
            .agents
            .find_by_name("launch-probe-ct-bad")
            .await
            .unwrap()
            .expect("launch_with_command persists its probe agent");
        let resp = SpawnOrchestratorResponse {
            agent: super::super::spawn::orchestrator_response(agent),
            token: String::new(),
            launch_status: bad.launch_status.clone(),
            launch_detail: bad.launch_detail.clone(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["launch_status"], "launch_failed");
        assert!(json["launch_detail"].is_string());
    }

    /// Seed the test workspace with a local compute target running `command`,
    /// then launch a bare agent through `launch_orchestrator_process` (the
    /// exact function the spawn cores call). The target's `target_type`
    /// variant is irrelevant to command resolution: `config["command"]`
    /// takes priority over every env fallback, and the dispatch keys on the
    /// lowercased string ("kubernetes" -> LocalTarget).
    async fn launch_with_command(
        state: &std::sync::Arc<crate::AppState>,
        ct_id: &str,
        command: &str,
    ) -> LaunchOutcome {
        let mut ct = gyre_domain::ComputeTargetEntity::new(
            Id::new(ct_id),
            Id::new("t1"),
            "test-target",
            gyre_domain::ComputeTargetType::Kubernetes,
            0,
        );
        ct.config = serde_json::json!({ "command": command });
        state.compute_targets.create(&ct).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();

        let agent = gyre_domain::Agent::new(Id::new(ct_id), format!("launch-probe-{ct_id}"), 0);
        state.agents.create(&agent).await.unwrap();
        launch_orchestrator_process(state, &agent, &ws, "tok").await
    }
}
