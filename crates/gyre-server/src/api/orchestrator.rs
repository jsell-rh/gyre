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

#[derive(Deserialize, Default)]
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
/// exactly what the spec forbids. (Supersedes this branch's earlier
/// advisory-only `warn_unapproved_persona`: the upstream base makes the
/// check genuinely enforcing, which is the stronger fix for the same
/// inert-enforcement finding.)
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
    /// Label of the compute target the launch resolved to (workspace
    /// assignment -> tenant default -> local); same convention as the
    /// worker path's AgentLaunchResult::compute_target_label. Carried on
    /// the outcome so the agent.spawned analytics event reports the target
    /// the process actually launched on, not a hardcoded value.
    pub compute_target_label: String,
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
    let target_config: Option<super::compute::ComputeTargetConfig> =
        workspace_target.map(|e| super::compute::ComputeTargetConfig {
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

    // Label of the resolved target for the outcome/analytics: the target
    // entity's id, or "local" when no workspace/tenant target is
    // configured (same convention as api/spawn.rs). Computed once from the
    // resolved config so every return path — including the fail-closed
    // early returns below, which happen after resolution — reports the
    // target that was selected for this launch.
    let compute_target_label = target_config
        .as_ref()
        .map(|cfg| cfg.id.clone())
        .unwrap_or_else(|| "local".to_string());

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
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Agent context env vars (agent-protocol.md M24). The orchestrator's
    // "task" claim is its own id (orchestrators are not task-bound).
    let mut env = std::collections::HashMap::new();
    env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
    env.insert("GYRE_AUTH_TOKEN".to_string(), token.to_string());
    env.insert("GYRE_AGENT_ID".to_string(), agent.id.to_string());
    env.insert("GYRE_TASK_ID".to_string(), agent.id.to_string());
    env.insert(
        "GYRE_ORCHESTRATOR".to_string(),
        agent.orchestrator_type.to_string(),
    );
    if let Some(rid) = &agent.repo_id {
        env.insert("GYRE_REPO_ID".to_string(), rid.to_string());
    }
    env.insert(
        "GYRE_WORKSPACE_ID".to_string(),
        agent.workspace_id.to_string(),
    );

    // platform-model.md §4.3: inject the four system prompts. The persona
    // prompt comes from the persona bound at orchestrator-spawn time
    // (fail-closed resolution in spawn_orchestrator); orchestrators are not
    // task-bound and have no worktree, so their context prompt says so
    // honestly instead of fabricating a task.
    let prompt_scope = crate::api::prompts::PromptScope {
        tenant_id: Some(workspace.tenant_id.clone()),
        workspace_id: workspace.id.clone(),
        repo_id: agent.repo_id.clone(),
        task_id: Some(agent.id.clone()),
        agent_id: Some(agent.id.clone()),
    };
    if let Some(persona_id) = &agent.persona_id {
        match state.personas.find_by_id(persona_id).await {
            Ok(Some(persona)) => {
                // Fail-closed (review F1, task-150): a persona file push
                // (git_http.rs) resets an Approved persona to Pending while
                // swapping its system_prompt, and the stale-agent detector
                // restarts a dead orchestrator with the preserved
                // persona_id. Injecting the stored prompt here would run
                // the replacement under content no human approved — the
                // exact scenario relaunch_agent_session refuses for
                // workers — so the launch fails instead.
                if persona.approval_status != gyre_domain::PersonaApprovalStatus::Approved {
                    let detail = format!(
                        "persona '{}' bound to orchestrator {} is {:?}; \
                         a human must approve it before the orchestrator can \
                         (re)launch",
                        persona.slug, agent.id, persona.approval_status
                    );
                    tracing::error!(
                        agent_id = %agent.id,
                        persona_id = %persona_id,
                        approval_status = ?persona.approval_status,
                        "orchestrator launch: bound persona not Approved; failing launch"
                    );
                    return LaunchOutcome {
                        launch_status: "launch_failed".to_string(),
                        launch_detail: Some(detail),
                        compute_target_label,
                    };
                }
                env.insert("GYRE_PROMPT_PERSONA".to_string(), persona.system_prompt);
            }
            _ => {
                tracing::warn!(
                    agent_id = %agent.id,
                    persona_id = %persona_id,
                    "bound persona unresolvable; skipping GYRE_PROMPT_PERSONA injection"
                );
            }
        }
    }
    env.insert(
        "GYRE_PROMPT_PROTOCOL".to_string(),
        crate::api::prompts::protocol_prompt().to_string(),
    );
    let orch_repo = match &agent.repo_id {
        Some(rid) => state.repos.find_by_id(rid).await.ok().flatten(),
        None => None,
    };
    env.insert(
        "GYRE_PROMPT_CONTEXT".to_string(),
        crate::api::prompts::context_prompt_for_orchestrator(orch_repo.as_ref()),
    );
    env.insert(
        "GYRE_PROMPT_CONSTRAINTS".to_string(),
        crate::api::prompts::constraints_prompt(state, &prompt_scope).await,
    );

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
            let image = cfg.config["image"]
                .as_str()
                .unwrap_or("gyre-agent:latest")
                .to_string();
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
            let host = cfg.config["host"]
                .as_str()
                .unwrap_or("localhost")
                .to_string();
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
                        compute_target_label,
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
                                    let mut probe = gyre_adapters::compute::SshTarget::new(
                                        user.clone(),
                                        host.clone(),
                                    );
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
                            if a.status == AgentStatus::Active && orch_type == a.orchestrator_type {
                                // Orchestrator process died; mark Dead so the
                                // stale detector spawns a replacement.
                                let _ = a.transition_status(AgentStatus::Dead);
                                let _ = state_mon.agents.update(&a).await;
                                // Auto-track agent.failed (analytics.md
                                // §Auto-Emitted Events: "Agent fails or is
                                // killed") — the process death is the kill,
                                // same event the endpoint death paths emit.
                                crate::api::analytics::record_agent_failed(
                                    &state_mon,
                                    &a,
                                    "orchestrator process died",
                                )
                                .await;
                            }
                        }
                        break;
                    }
                }
            });
            LaunchOutcome {
                launch_status: "running".to_string(),
                launch_detail: None,
                compute_target_label,
            }
        }
        Err(e) => LaunchOutcome {
            launch_status: "launch_failed".to_string(),
            launch_detail: Some(format!("{e}")),
            compute_target_label,
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
    if let Ok(Some(existing)) = state.agents.find_by_name(&agent.name).await {
        if is_live(&existing) {
            return Err(ApiError::InvalidInput(format!(
                "an agent named '{}' already exists; choose a different name",
                agent.name
            )));
        }
        // Resume semantics (§8): the scope-slot checks above already
        // guarantee no LIVE orchestrator occupies this scope, so a name
        // held by a Dead/Stopped/Failed agent is a stopped predecessor
        // from a failed run — typically the bootstrap default name
        // '{repo}-orchestrator' colliding with itself across runs
        // (round-3 finding 4). Release the name by suffixing the dead
        // row (same scheme the stale detector's restart path uses for
        // replacements) instead of rejecting the re-run; the agent id,
        // not the name, is the durable identity.
        let base = existing
            .name
            .split("-superseded-")
            .next()
            .unwrap_or(&existing.name)
            .to_string();
        let mut n = 1;
        let mut free = format!("{base}-superseded-{n}");
        while let Ok(Some(_)) = state.agents.find_by_name(&free).await {
            n += 1;
            free = format!("{base}-superseded-{n}");
        }
        let mut renamed = existing;
        renamed.name = free;
        state.agents.update(&renamed).await?;
        tracing::info!(
            agent_id = %renamed.id,
            new_name = %renamed.name,
            "released orchestrator name held by a stopped predecessor"
        );
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

    // Auto-track spawn (analytics.md §Auto-Emitted Events). Emitted after
    // the launch so the spec-required compute_target property reports the
    // target the process actually launched on (review f3f37604: previously
    // a hardcoded "local", wrong for any workspace/tenant compute target) —
    // the same shape as the worker path, which records
    // launch.compute_target_label.
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
            // Orchestrators are not task-bound (the JWT task_id claim is the
            // orchestrator's own id only for gyre_agent_complete compat) —
            // the spec's task_id property is null here, not a fabricated id.
            "task_id": serde_json::Value::Null,
            "compute_target": launch.compute_target_label.clone(),
            "persona": persona_name,
            "orchestrator_type": orchestrator_type.to_string(),
            "workspace_id": workspace_id.to_string(),
            "repo_id": repo_id.map(|r| r.to_string()),
        }),
        now,
    )
    .with_scope(None, None, Some(workspace_id), repo_id);
    let _ = state.analytics.record(&ev).await;

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

    let persona =
        resolve_orchestrator_persona(state, "workspace-orchestrator", &ws_id, None).await?;

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
/// only cross-tenant containment) — for EVERY caller, including the
/// global system token. Unlike the repo-tier spawn route below, no
/// bootstrap flow spawns a workspace orchestrator (platform-model.md §8
/// step 8 spawns the repo orchestrator only), so no operational path
/// needs the global token to cross tenants here: containment must hold
/// for every caller or this registry-exempt route could mint a
/// workspace-scoped orchestrator JWT for a tenant the caller does not
/// belong to (task-093 F2, pinned by the
/// `orchestrator_spawn_denied_across_tenants` regression test).
pub async fn spawn_workspace_orchestrator(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Json(req): Json<SpawnOrchestratorRequest>,
) -> Result<(StatusCode, Json<SpawnOrchestratorResponse>), ApiError> {
    // Tenant containment: load the workspace, compare its tenant against
    // the caller's resolved tenant. No system-principal bypass: the global
    // token's tenant is the bootstrap principal ("default"), and no
    // documented flow spawns a workspace orchestrator outside it.
    let ws_id = Id::new(workspace_id.clone());
    let workspace = state
        .workspaces
        .find_by_id(&ws_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {workspace_id} not found")))?;
    if auth.tenant_id != workspace.tenant_id.to_string() {
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

    let persona =
        resolve_orchestrator_persona(state, "repo-orchestrator", &ws_id, Some(&rid)).await?;

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
/// bootstrap principal, not a tenant scope, and the documented dev-mode
/// bootstrap (`gyre bootstrap --dev`, docs/cli.md) authenticates this
/// route's step-8 repo-orchestrator spawn with that static token against
/// the freshly created "dev" tenant. (The workspace-tier spawn route above
/// has NO bypass: no bootstrap flow spawns a workspace orchestrator, so
/// its containment holds for every caller.)
pub async fn spawn_repo_orchestrator(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    Json(req): Json<SpawnOrchestratorRequest>,
) -> Result<(StatusCode, Json<SpawnOrchestratorResponse>), ApiError> {
    // G6: ABAC enforcement - repo access required to spawn its orchestrator.
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/orchestrator/spawn",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }

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
        if !auth.is_system_principal() && auth.tenant_id != ws.tenant_id.to_string() {
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

        let (agent, token, _launch) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(None), "user-1")
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
    async fn repo_orchestrator_response_serializes_no_key_twice() {
        // Round-3 finding 1 regression: `OrchestratorAgentResponse` flattened
        // `AgentResponse` (which carries `persona_id`) while also declaring
        // `persona_id` itself, so serde emitted the key twice on the WIRE.
        // `serde_json::to_value` collapses duplicates (Map insert), which is
        // why the repo_id test above could not catch it — this test asserts
        // on the serialized STRING, where duplicates survive, and then
        // strict-deserializes it the way the CLI does (serde_derive rejects
        // duplicate fields).
        let state = test_state();
        seed(&state).await;
        let (agent, _token, _launch) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("wire-check")), "user-1")
                .await
                .unwrap();
        let text = serde_json::to_string(&orchestrator_response(agent)).unwrap();

        // No top-level key may appear twice, for ANY key — not just the ones
        // a previous regression knew about.
        let keys = top_level_keys(&text);
        let mut seen = std::collections::HashSet::new();
        for k in &keys {
            assert!(seen.insert(k.clone()), "key '{k}' appears twice: {text}");
        }

        // persona_id is carried exactly once (by the flattened AgentResponse)
        // with the resolved persona binding.
        assert_eq!(keys.iter().filter(|k| *k == "persona_id").count(), 1);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["persona_id"], "repo-orchestrator-persona");

        // The CLI's strict shape must parse the wire bytes (this is the
        // exact failure every first-run bootstrap hit: `duplicate field
        // 'persona_id'`).
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct WireAgent {
            id: String,
            #[serde(default)]
            repo_id: Option<String>,
            #[serde(default)]
            persona_id: Option<String>,
            orchestrator_type: Option<String>,
            restart_on_failure: Option<bool>,
        }
        let parsed: WireAgent = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed.persona_id.as_deref(),
            Some("repo-orchestrator-persona")
        );
        assert_eq!(parsed.repo_id.as_deref(), Some("r-1"));
    }

    /// Extract the top-level object keys of a JSON object literal in document
    /// order, duplicates included. Values may be scalars or nested
    /// objects/arrays; only depth-1 keys are collected. Used by the
    /// duplicate-key regression test — `serde_json::Value` cannot represent
    /// duplicate keys (its Map inserts over them), so duplicates are only
    /// visible in the text.
    fn top_level_keys(json: &str) -> Vec<String> {
        let chars: Vec<char> = json.chars().collect();
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            match c {
                '"' => in_string = true,
                '{' | '[' => {
                    depth += 1;
                    if c == '{' && depth == 1 {
                        // The top-level object's first key follows its brace.
                        i += 1;
                        i = skip_ws(&chars, i);
                        if i < chars.len() && chars[i] == '"' {
                            let (key, next) = read_json_string(&chars, i);
                            keys.push(key);
                            i = next;
                            continue;
                        }
                        continue;
                    }
                }
                '}' | ']' => depth = depth.saturating_sub(1),
                ',' if depth == 1 => {
                    // In an object, a key follows every top-level comma.
                    i += 1;
                    i = skip_ws(&chars, i);
                    if i < chars.len() && chars[i] == '"' {
                        let (key, next) = read_json_string(&chars, i);
                        keys.push(key);
                        i = next;
                        continue;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        keys
    }

    fn skip_ws(chars: &[char], mut i: usize) -> usize {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    /// Read a JSON string starting at `chars[start] == '"'`; returns the
    /// decoded-ish content and the index just past the closing quote.
    fn read_json_string(chars: &[char], start: usize) -> (String, usize) {
        let mut s = String::new();
        let mut i = start + 1;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                s.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                return (s, i + 1);
            }
            s.push(c);
            i += 1;
        }
        (s, i)
    }

    #[tokio::test]
    async fn spawn_rejects_missing_persona() {
        // Fail-closed persona resolution (round-3 finding 5): a persona
        // absent from the repo/workspace/tenant chain must reject the spawn
        // with 404 — never fall back to an unstyled orchestrator.
        let state = test_state();
        seed(&state).await;
        // Remove the personas seed leaves nothing in the scope chain.
        for slug in ["workspace-orchestrator", "repo-orchestrator"] {
            let p = state
                .personas
                .find_by_slug_and_scope(slug, &gyre_domain::PersonaScope::Tenant(Id::new("t1")))
                .await
                .unwrap()
                .expect("seeded persona");
            state.personas.delete(&p.id).await.unwrap();
        }

        let err = spawn_repo_orchestrator_core(&state, "r-1", req(Some("no-persona")), "user-1")
            .await
            .unwrap_err();
        match &err {
            ApiError::NotFound(msg) => {
                assert!(msg.contains("repo-orchestrator"), "got: {msg}");
            }
            other => panic!("expected NotFound, got: {other}"),
        }
        // Nothing was persisted — fail-closed means no orphan agent row.
        assert!(state
            .agents
            .find_by_name("no-persona")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn spawn_rejects_unapproved_persona() {
        // Fail-closed persona resolution: a persona that resolves but is not
        // Approved must reject the spawn with a conflict — a Pending persona
        // must not style an orchestrator that then runs unreviewed.
        let state = test_state();
        seed(&state).await;
        let mut pending = state
            .personas
            .find_by_slug_and_scope(
                "repo-orchestrator",
                &gyre_domain::PersonaScope::Tenant(Id::new("t1")),
            )
            .await
            .unwrap()
            .expect("seeded persona");
        pending.approval_status = gyre_domain::PersonaApprovalStatus::Pending;
        state.personas.update(&pending).await.unwrap();

        let err = spawn_repo_orchestrator_core(&state, "r-1", req(Some("unapproved")), "user-1")
            .await
            .unwrap_err();
        match &err {
            ApiError::Conflict(msg) => {
                assert!(msg.contains("repo-orchestrator"), "got: {msg}");
            }
            other => panic!("expected Conflict, got: {other}"),
        }
        assert!(state
            .agents
            .find_by_name("unapproved")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn spawn_frees_name_of_stopped_predecessor() {
        // Round-3 finding 4a: a failed bootstrap run leaves a stopped
        // (Dead/Stopped/Failed) orchestrator holding the default name; the
        // scope-slot check already guarantees no LIVE orchestrator holds the
        // scope, so the re-run must release the name from the dead row
        // instead of failing with 400 "already exists".
        let state = test_state();
        seed(&state).await;

        let (first, _t, _l) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("gyre-orchestrator")), "user-1")
                .await
                .unwrap();
        kill(&state, "gyre-orchestrator").await;

        let (second, _t2, _l2) =
            spawn_repo_orchestrator_core(&state, "r-1", req(Some("gyre-orchestrator")), "user-1")
                .await
                .unwrap();
        assert_eq!(second.name, "gyre-orchestrator");
        assert_eq!(second.status, AgentStatus::Active);

        // The dead predecessor keeps its identity under a suffixed name.
        let renamed = state.agents.find_by_id(&first.id).await.unwrap().unwrap();
        assert_eq!(renamed.name, "gyre-orchestrator-superseded-1");
        assert_eq!(renamed.status, AgentStatus::Dead);

        // A LIVE holder still rejects with 400 (different-repo name steal
        // must not be possible).
        spawn_repo_orchestrator_core(&state, "r-2", req(Some("gyre-orchestrator")), "user-1")
            .await
            .unwrap_err();
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

    /// Review finding F3 regression (task-115): server-driven
    /// orchestrators (spawned_by="system", created by the signal chain)
    /// have no external process to heartbeat. The stale-agent detector
    /// must refresh their heartbeat instead of killing them — the
    /// pre-fix behavior marked each one Dead, restarted an Active
    /// replacement that equally never heartbeated, and churned
    /// spawn/kill/restart forever while leaking budget slots.
    #[tokio::test]
    async fn stale_detector_refreshes_system_orchestrator_instead_of_churning() {
        let state = test_state();
        seed(&state).await;

        // System-spawned workspace orchestrator (as the signal chain does).
        let (agent, _token, _launch) = spawn_workspace_orchestrator_core(
            &state,
            "ws-1",
            req(Some("system-ws-orch")),
            "system",
        )
        .await
        .unwrap();

        // Age it past the heartbeat timeout — no process will ever
        // heartbeat for it.
        let mut aged = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        aged.spawned_at = aged.spawned_at.saturating_sub(10_000);
        aged.last_heartbeat = None;
        state.agents.update(&aged).await.unwrap();

        crate::stale_agents::run_once(&state).await.unwrap();

        // The SAME agent is refreshed and stays Active — not Dead, and no
        // restart replacement was spawned.
        let refreshed = state.agents.find_by_id(&agent.id).await.unwrap().unwrap();
        assert_eq!(
            refreshed.status,
            AgentStatus::Active,
            "system orchestrator must be refreshed, not marked Dead"
        );
        assert!(
            refreshed.last_heartbeat.is_some(),
            "detector stamped a fresh heartbeat on the system orchestrator"
        );

        let peers = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        assert!(
            !peers
                .iter()
                .any(|a| a.name.starts_with("system-ws-orch-restart")),
            "no restart replacement spawned for a system orchestrator"
        );
        assert_eq!(
            peers
                .iter()
                .filter(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
                .count(),
            1,
            "exactly one live workspace orchestrator (no churn)"
        );

        // A second cycle must keep refreshing the same agent — the churn
        // loop the finding describes is spawn→kill→restart→…, so prove
        // two consecutive detector cycles never grow the orchestrator set.
        crate::stale_agents::run_once(&state).await.unwrap();
        let after = state
            .agents
            .list_by_workspace(&Id::new("ws-1"))
            .await
            .unwrap();
        assert_eq!(
            after
                .iter()
                .filter(|a| a.orchestrator_type == OrchestratorType::WorkspaceOrchestrator)
                .count(),
            1,
            "still exactly one live workspace orchestrator after a second cycle"
        );
        assert_eq!(
            state
                .agents
                .find_by_id(&agent.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            AgentStatus::Active
        );
    }

    #[tokio::test]
    async fn orchestrator_spawn_emits_agent_spawned_analytics_event() {
        // analytics.md §Auto-Emitted Events: the orchestrator spawn path
        // reports its tier via the persona property.
        let state = test_state();
        seed(&state).await;

        let (agent, _token, _launch) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(None), "user-1")
                .await
                .unwrap();

        let events = state
            .analytics
            .query(Some("agent.spawned"), None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "one agent.spawned event expected");
        let ev = &events[0];
        assert_eq!(ev.agent_id.as_deref(), Some(agent.id.as_str()));
        assert_eq!(ev.properties["persona"], "workspace-orchestrator");
        assert_eq!(ev.properties["orchestrator_type"], "workspace_orchestrator");
        // No workspace/tenant compute target is configured in this test, so
        // the honest resolution of the priority chain (workspace assignment
        // -> tenant default -> local) IS "local" here. The configured-target
        // case is pinned by
        // orchestrator_spawn_analytics_reports_resolved_compute_target.
        assert_eq!(ev.properties["compute_target"], "local");
        // Orchestrators are not task-bound: task_id must be null, not the
        // agent's own id (regression: previously fabricated from agent.id).
        assert!(ev.properties["task_id"].is_null());
        assert_eq!(ev.workspace_id.as_deref(), Some("ws-1"));
    }

    /// Review f3f37604 regression: the spec-required `agent.spawned`
    /// property `compute_target` must report the target the orchestrator
    /// process actually launched on (workspace assignment -> tenant
    /// default -> local), not a hardcoded "local". The pre-fix defect
    /// recorded "local" while the process launched on the configured
    /// workspace target — silently wrong in every deployment that
    /// configures one.
    #[tokio::test]
    async fn orchestrator_spawn_analytics_reports_resolved_compute_target() {
        let state = test_state();
        seed(&state).await;

        // Configure a workspace compute target — the exact deployment
        // shape launch_with_command / orchestrator_launch_injects_system_prompts
        // use. Kubernetes-typed targets dispatch via LocalTarget, so
        // /bin/true launches deterministically in the test environment.
        let mut ct = gyre_domain::ComputeTargetEntity::new(
            Id::new("ct-analytics-resolved"),
            Id::new("t1"),
            "analytics-resolved-target",
            gyre_domain::ComputeTargetType::Kubernetes,
            0,
        );
        ct.config = serde_json::json!({ "command": "/bin/true" });
        state.compute_targets.create(&ct).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();

        let (agent, _token, launch) =
            spawn_workspace_orchestrator_core(&state, "ws-1", req(None), "user-1")
                .await
                .unwrap();
        assert_eq!(
            launch.launch_status, "running",
            "launch failed: {:?}",
            launch.launch_detail
        );

        let events = state
            .analytics
            .query(Some("agent.spawned"), None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "one agent.spawned event expected");
        let ev = &events[0];
        assert_eq!(ev.agent_id.as_deref(), Some(agent.id.as_str()));
        // The event must carry the RESOLVED target id, proving the property
        // is looked up from the launch resolution, not hardcoded. This
        // assertion fails against the pre-fix code (which recorded "local").
        assert_eq!(
            ev.properties["compute_target"], "ct-analytics-resolved",
            "agent.spawned must report the compute target the process \
             actually launched on"
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

    #[tokio::test]
    async fn orchestrator_launch_injects_system_prompts() {
        let state = test_state();
        seed(&state).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp
            .path()
            .join("orch-prompt-dump")
            .to_string_lossy()
            .into_owned();
        let script_path = format!("{dump_path}.sh");
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\nenv | sort > {dump_path}\nexit 0\n"),
        )
        .expect("writing orch prompt-dump script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod orch prompt-dump script");
        }

        let mut ct = gyre_domain::ComputeTargetEntity::new(
            Id::new("ct-orch-prompts"),
            Id::new("t1"),
            "orch-prompt-target",
            gyre_domain::ComputeTargetType::Kubernetes,
            0,
        );
        ct.config = serde_json::json!({ "command": script_path });
        state.compute_targets.create(&ct).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();

        // Launch agent with the seeded (approved) workspace-orchestrator
        // persona bound — exactly what spawn_workspace_orchestrator_core
        // persists via resolve_orchestrator_persona.
        let mut agent =
            gyre_domain::Agent::new(Id::new("orch-prompt-agent"), "orch-prompt-agent", 0);
        agent.workspace_id = Id::new("ws-1");
        agent.persona_id = Some(Id::new("workspace-orchestrator-persona"));
        agent.orchestrator_type = OrchestratorType::WorkspaceOrchestrator;
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();

        let launch = launch_orchestrator_process(&state, &agent, &ws, "tok").await;
        assert_eq!(
            launch.launch_status, "running",
            "launch failed: {:?}",
            launch.launch_detail
        );

        // Bounded wait for the dump file (child process writes it).
        let mut dump = None;
        for _ in 0..250 {
            if let Ok(content) = std::fs::read_to_string(&dump_path) {
                if content.ends_with('\n') {
                    dump = Some(content);
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let dump = dump.expect("orch prompt-dump script should have written its output");
        assert!(
            dump.contains("GYRE_PROMPT_PERSONA=test orchestrator persona"),
            "persona prompt must be injected from the bound persona:\n{dump}"
        );
        assert!(
            dump.contains("GYRE_PROMPT_PROTOCOL=# Gyre Agent Protocol"),
            "protocol prompt must be injected:\n{dump}"
        );
        assert!(
            dump.contains("GYRE_PROMPT_CONTEXT=# Agent Context"),
            "context prompt must be injected:\n{dump}"
        );
        assert!(
            dump.contains("No task is bound to this agent"),
            "orchestrator context must record its unbound task state honestly:\n{dump}"
        );
        assert!(
            dump.contains("GYRE_PROMPT_CONSTRAINTS=# Agent Constraints"),
            "constraints prompt must be injected:\n{dump}"
        );
        assert!(
            dump.contains("Workspace: ws-1"),
            "constraints prompt must carry the workspace boundary:\n{dump}"
        );
    }

    /// Review finding F1 regression (task-150): a persona file push
    /// (git_http.rs) resets an Approved persona to Pending while swapping
    /// its system_prompt, and the stale-agent detector restarts a dead
    /// orchestrator with the preserved persona_id by calling
    /// `launch_orchestrator_process` directly. The launch must fail closed
    /// — never inject the new, human-unapproved prompt content — mirroring
    /// what relaunch_agent_session refuses for workers.
    #[tokio::test]
    async fn orchestrator_launch_fails_closed_on_unapproved_persona() {
        let state = test_state();
        seed(&state).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp
            .path()
            .join("orch-unapproved-dump")
            .to_string_lossy()
            .into_owned();
        let script_path = format!("{dump_path}.sh");
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\nenv | sort > {dump_path}\nexit 0\n"),
        )
        .expect("writing orch unapproved-dump script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod orch unapproved-dump script");
        }

        let mut ct = gyre_domain::ComputeTargetEntity::new(
            Id::new("ct-orch-unapproved"),
            Id::new("t1"),
            "orch-unapproved-target",
            gyre_domain::ComputeTargetType::Kubernetes,
            0,
        );
        ct.config = serde_json::json!({ "command": script_path });
        state.compute_targets.create(&ct).await.unwrap();
        let mut ws = state
            .workspaces
            .find_by_id(&Id::new("ws-1"))
            .await
            .unwrap()
            .unwrap();
        ws.compute_target_id = Some(ct.id.clone());
        state.workspaces.update(&ws).await.unwrap();

        // The restart path's preserved persona binding: the seeded
        // (approved) workspace-orchestrator persona.
        let mut agent =
            gyre_domain::Agent::new(Id::new("orch-unapproved-agent"), "orch-unapproved-agent", 0);
        agent.workspace_id = Id::new("ws-1");
        agent.persona_id = Some(Id::new("workspace-orchestrator-persona"));
        agent.orchestrator_type = OrchestratorType::WorkspaceOrchestrator;
        agent.transition_status(AgentStatus::Active).unwrap();
        state.agents.create(&agent).await.unwrap();

        // Simulate the persona-file push that git_http.rs applies: the
        // bound persona's system_prompt is swapped and approval resets to
        // Pending (approved_by/approved_at cleared).
        let mut pushed = state
            .personas
            .find_by_id(&Id::new("workspace-orchestrator-persona"))
            .await
            .unwrap()
            .expect("seeded persona");
        pushed.system_prompt = "UNAPPROVED PUSHED PROMPT CONTENT".to_string();
        pushed.refresh_content_hash();
        pushed.approval_status = gyre_domain::PersonaApprovalStatus::Pending;
        pushed.approved_by = None;
        pushed.approved_at = None;
        state.personas.update(&pushed).await.unwrap();

        // The exact call stale_agents.rs restart_orchestrator makes.
        let launch = launch_orchestrator_process(&state, &agent, &ws, "tok").await;
        assert_eq!(
            launch.launch_status, "launch_failed",
            "unapproved bound persona must fail the orchestrator launch"
        );
        let detail = launch
            .launch_detail
            .as_deref()
            .expect("launch_failed must carry a reason");
        assert!(
            detail.contains("Pending"),
            "failure detail must name the approval state: {detail}"
        );

        // Fail-closed means no process at all: the dump script must never
        // have run, so no environment carrying the unapproved prompt
        // content was ever created. (The pre-fix defect would have written
        // the dump with GYRE_PROMPT_PERSONA=UNAPPROVED PUSHED PROMPT CONTENT.)
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !std::path::Path::new(&dump_path).exists(),
            "no orchestrator process may be spawned with an unapproved persona"
        );
    }
}
