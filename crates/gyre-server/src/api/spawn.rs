use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, AgentWorktree, AnalyticsEvent, DisconnectedBehavior, MergeRequest,
    TaskStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::{
    auth::AuthenticatedAgent, container_audit, domain_events::DomainEvent, git_refs,
    workload_attestation, AppState,
};

use super::agents::AgentResponse;
use super::error::ApiError;
use super::merge_requests::MrResponse;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SpawnAgentRequest {
    pub name: String,
    pub repo_id: String,
    pub task_id: String,
    pub branch: String,
    pub parent_id: Option<String>,
    /// Optional compute target to associate with this agent spawn.
    pub compute_target_id: Option<String>,
    /// How the agent behaves when the server becomes unreachable (BCP graceful degradation).
    pub disconnected_behavior: Option<DisconnectedBehavior>,
}

#[derive(Serialize)]
pub struct SpawnAgentResponse {
    pub agent: AgentResponse,
    pub token: String,
    pub worktree_path: String,
    pub clone_url: String,
    pub branch: String,
    pub compute_target_id: Option<String>,
    /// jj change ID created for this agent's worktree, if jj was successfully initialized.
    pub jj_change_id: Option<String>,
    /// Container ID when the agent was spawned via a container compute target (M19.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// SHA256 of the workspace's meta-spec set at spawn time, for provenance (M32).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_spec_set_sha: Option<String>,
}

#[derive(Deserialize)]
pub struct CompleteAgentRequest {
    pub branch: String,
    pub title: String,
    pub target_branch: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/agents/spawn
///
/// Orchestrated agent provisioning in one call:
/// 1. Creates agent record (Active status, sets parent_id)
/// 2. Generates auth token
/// 3. Creates a git worktree on the repo for the agent's branch
/// 4. Assigns the task to the agent, advances task to InProgress
/// 5. Records the worktree in DB (linked to agent + task)
#[instrument(skip(state, auth, req), fields(agent_name = %req.name, branch = %req.branch))]
pub async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<SpawnAgentRequest>,
) -> Result<(StatusCode, Json<SpawnAgentResponse>), ApiError> {
    // Verify repo exists
    let repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    crate::abac::check_repo_abac(&state, &req.repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;

    // M22.2: Budget enforcement — check workspace concurrent-agent and daily limits.
    super::budget::check_spawn_budget(&state, &repo.project_id.to_string())
        .await
        .map_err(ApiError::TooManyRequests)?;

    // Verify task exists
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&req.task_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {} not found", req.task_id)))?;

    // Validate compute target if provided
    if let Some(ref ct_id) = req.compute_target_id {
        if state
            .kv_store
            .kv_get("compute_targets", ct_id.as_str())
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return Err(ApiError::NotFound(format!(
                "compute target {ct_id} not found"
            )));
        }
    }

    let now = now_secs();

    // Create agent with Active status
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    agent.spawned_by = Some(auth.agent_id.clone());
    if let Some(behavior) = req.disconnected_behavior {
        agent.disconnected_behavior = behavior;
    }
    agent.assign_task(Id::new(&req.task_id));
    agent.workspace_id = repo.workspace_id.clone();
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.create(&agent).await?;

    // Pre-mint a JWT without workload claims so it can be injected into the
    // container environment at spawn time.  After spawn we create the workload
    // attestation record (stored in state.workload_attestations) which is
    // queryable via GET /api/v1/agents/{id}/workload.
    let token = state
        .agent_signing_key
        .mint(
            &agent.id.to_string(),
            &req.task_id,
            &auth.agent_id,
            &state.base_url,
            state.agent_jwt_ttl_secs,
        )
        .unwrap_or_else(|e| {
            tracing::error!("JWT pre-mint failed, falling back to UUID token: {e}");
            uuid::Uuid::new_v4().to_string()
        });
    // Store now so the container can authenticate immediately upon start.
    let _ = state
        .kv_store
        .kv_set("agent_tokens", &agent.id.to_string(), token.clone())
        .await;

    // Compute worktree path: {repo_path}/worktrees/{branch_slug}
    let branch_slug = req.branch.replace('/', "-");
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch_slug);

    // Create git worktree. The adapter tries existing branch first, then
    // creates a new branch from HEAD if the branch doesn't exist yet.
    if let Err(e) = state
        .git_ops
        .create_worktree(&repo.path, &worktree_path, &req.branch)
        .await
    {
        let msg = e.to_string();
        let msg_lc = msg.to_lowercase();
        if msg_lc.contains("not a valid object") || msg_lc.contains("bad default revision") {
            return Err(ApiError::InvalidInput(format!(
                "cannot create worktree: repo has no commits yet — push an initial commit before spawning (branch: {})",
                req.branch
            )));
        }
        tracing::warn!("create_worktree failed (non-fatal): {e}");
    }

    // Initialize jj in the worktree and create an initial change (best-effort).
    // Only attempted if the worktree directory exists on disk.
    let jj_change_id = if std::path::Path::new(&worktree_path).exists() {
        match state.jj_ops.jj_init(&worktree_path).await {
            Ok(()) => {
                let description = format!("Agent {}: task {}", agent.name, req.task_id);
                match state.jj_ops.jj_new(&worktree_path, &description).await {
                    Ok(change_id) => {
                        tracing::debug!(
                            agent_id = %agent.id,
                            change_id = %change_id,
                            "jj initialized in worktree"
                        );
                        Some(change_id)
                    }
                    Err(e) => {
                        tracing::debug!(agent_id = %agent.id, "jj new skipped: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                tracing::debug!(agent_id = %agent.id, "jj init skipped: {e}");
                None
            }
        }
    } else {
        None
    };

    // Write custom ref namespaces (best-effort)
    if let Some(sha) = git_refs::resolve_ref(&repo.path, "HEAD").await {
        let agent_ref = format!("refs/agents/{}/head", agent.id);
        let ralph_ref = format!("refs/ralph/{}/implement", task.id);
        git_refs::write_ref(&repo.path, &agent_ref, &sha).await;
        git_refs::write_ref(&repo.path, &ralph_ref, &sha).await;
    }

    // Record worktree in DB linked to agent and task
    let wt = AgentWorktree::new(
        new_id(),
        agent.id.clone(),
        Id::new(&req.repo_id),
        Some(Id::new(&req.task_id)),
        req.branch.clone(),
        worktree_path.clone(),
        now,
    );
    state.worktrees.create(&wt).await?;

    // Assign task to agent and advance to InProgress
    task.assigned_to = Some(agent.id.clone());
    if task.status == TaskStatus::Backlog {
        let _ = task.transition_status(TaskStatus::InProgress);
    }
    task.updated_at = now;
    state.tasks.update(&task).await?;

    // Build clone URL: {base_url}/git/{project_id}/{repo_name}
    let clone_url = format!("{}/git/{}/{}", state.base_url, repo.project_id, repo.name);

    // M19.1: Resolve the effective compute target.
    // Priority: compute_target_id from request → GYRE_DEFAULT_COMPUTE_TARGET env → local.
    //
    // When compute_target_id points to a "container" type target, the agent process is
    // launched inside Docker/Podman with security defaults (G8-A/B/C).
    // When it points to an "ssh" type target with container_mode enabled (M19.5), the
    // docker command is executed on the remote SSH host.
    let compute_target_label = req.compute_target_id.as_deref().unwrap_or("local");

    // Resolve which target config to use.
    let resolved_target_config: Option<super::compute::ComputeTargetConfig> = {
        if let Some(ref ct_id) = req.compute_target_id {
            state
                .kv_store
                .kv_get("compute_targets", ct_id.as_str())
                .await
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            // Check GYRE_DEFAULT_COMPUTE_TARGET env var.
            let default_mode = std::env::var("GYRE_DEFAULT_COMPUTE_TARGET")
                .unwrap_or_else(|_| "local".to_string());
            if default_mode == "container" {
                // Find first container-type target (if any).
                state
                    .kv_store
                    .kv_list("compute_targets")
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find_map(|(_, v)| {
                        serde_json::from_str::<super::compute::ComputeTargetConfig>(&v)
                            .ok()
                            .filter(|t| t.target_type == "container")
                    })
            } else {
                None
            }
        }
    };

    // Launch a real process and monitor its lifecycle.
    // Capture the PID (local) or container ID (container) for workload attestation.
    let spawned_pid: Option<u32>;
    let spawned_container_id: Option<String>; // M19.1/M19.3/M19.4
    let spawned_container_image: Option<String>; // M19.3/M19.4

    {
        let effective_work_dir = if std::path::Path::new(&worktree_path).exists() {
            worktree_path.clone()
        } else {
            // For containers, use /workspace (absolute path required by Docker).
            "/workspace".to_string()
        };
        // Command is server-controlled only — never from user input (C-1 RCE fix).
        // Use compute target's configured command, or fall back to /gyre/entrypoint.sh
        let command = resolved_target_config
            .as_ref()
            .and_then(|cfg| cfg.config.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("/gyre/entrypoint.sh")
            .to_string();
        let args: Vec<String> = resolved_target_config
            .as_ref()
            .and_then(|cfg| cfg.config.get("args"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Inject agent context env vars so the container can bootstrap itself.
        let mut container_env = std::collections::HashMap::new();
        container_env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
        container_env.insert("GYRE_AUTH_TOKEN".to_string(), token.clone());
        container_env.insert("GYRE_CLONE_URL".to_string(), clone_url.clone());
        container_env.insert("GYRE_BRANCH".to_string(), req.branch.clone());
        container_env.insert("GYRE_AGENT_ID".to_string(), agent.id.to_string());
        container_env.insert("GYRE_TASK_ID".to_string(), req.task_id.clone());
        container_env.insert("GYRE_REPO_ID".to_string(), req.repo_id.clone());

        // M27: Inject operator-configured credentials via GYRE_CRED_* prefix.
        // The cred-proxy sidecar reads GYRE_CRED_* vars, stores in memory, and scrubs
        // them before the agent process starts — raw values are never in the agent env.
        // Format: KEY1=VALUE1,KEY2=VALUE2 (values may contain '=' — split on first '=' only).
        if let Ok(creds) = std::env::var("GYRE_AGENT_CREDENTIALS") {
            for pair in creds.split(',') {
                let pair = pair.trim();
                if let Some((k, v)) = pair.split_once('=') {
                    if !k.is_empty() {
                        container_env.insert(format!("GYRE_CRED_{k}"), v.to_string());
                    }
                }
            }
        }
        // GCP service account JSON (may contain commas — injected via a dedicated var).
        if let Ok(sa_json) = std::env::var("GYRE_AGENT_GCP_SA_JSON") {
            if !sa_json.is_empty() {
                container_env.insert("GYRE_CRED_GCP_SA_JSON".to_string(), sa_json);
            }
        }

        // M27: cred-proxy addresses for credential routing.
        container_env.insert(
            "GYRE_CRED_PROXY".to_string(),
            "http://127.0.0.1:8765".to_string(),
        );
        container_env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            "http://127.0.0.1:8765".to_string(),
        );
        // Placeholder so the Anthropic SDK initialises; cred-proxy injects the real key per request.
        container_env.insert("ANTHROPIC_API_KEY".to_string(), "proxy-managed".to_string());

        let spawn_config = gyre_ports::SpawnConfig {
            name: agent.name.clone(),
            command: command.clone(),
            args: args.clone(),
            env: container_env.clone(),
            work_dir: effective_work_dir.clone(),
        };

        match &resolved_target_config {
            Some(cfg) if cfg.target_type == "container" => {
                // M19.1: Spawn via ContainerTarget.
                let image = cfg.config["image"]
                    .as_str()
                    .unwrap_or("gyre-agent:latest")
                    .to_string();
                let mut ct = gyre_adapters::compute::ContainerTarget::new(image.clone());
                // Apply optional config overrides from the stored target config.
                // G8: default --network=none (secure). Compute target config
                // must explicitly set "network": "bridge" for agents that
                // need to reach the server (clone, heartbeat, complete).
                let network = cfg.config["network"].as_str().unwrap_or("none");
                ct = ct.with_network(network);
                if let Some(mem) = cfg.config["memory_limit"].as_str() {
                    ct = ct.with_memory_limit(mem);
                }
                if let Some(pids) = cfg.config["pids_limit"].as_u64() {
                    ct = ct.with_pids_limit(pids as u32);
                }
                if let Some(user) = cfg.config["user"].as_str() {
                    ct = ct.with_user(user);
                }

                match gyre_ports::ComputeTarget::spawn_process(&ct, &spawn_config).await {
                    Ok(handle) => {
                        spawned_pid = handle.pid;
                        spawned_container_id = Some(handle.id.clone());
                        spawned_container_image = Some(image.clone());

                        // M19.3: Capture container audit record (best-effort).
                        let runtime_str = cfg.config["runtime"]
                            .as_str()
                            .unwrap_or("docker")
                            .to_string();
                        let rec = container_audit::capture_spawn_audit(
                            &agent.id.to_string(),
                            &handle.id,
                            &image,
                            &runtime_str,
                        )
                        .await;
                        state
                            .container_audits
                            .lock()
                            .await
                            .insert(agent.id.to_string(), rec);

                        // M19.3: Emit AgentContainerSpawned domain event.
                        let _ = state.event_tx.send(DomainEvent::AgentContainerSpawned {
                            agent_id: agent.id.to_string(),
                            container_id: handle.id.clone(),
                            image: image.clone(),
                            runtime: runtime_str.clone(),
                        });

                        // M23: Emit container_started audit event.
                        {
                            let ctx = crate::container_audit::AuditCtx {
                                audit: state.audit.as_ref(),
                                broadcast_tx: &state.audit_broadcast_tx,
                            };
                            crate::container_audit::emit_started(
                                &ctx,
                                &agent.id.to_string(),
                                &handle.id,
                                &image,
                            )
                            .await;
                        }

                        let agent_id_str = agent.id.to_string();
                        state
                            .process_registry
                            .lock()
                            .await
                            .insert(agent_id_str.clone(), handle.clone());

                        // Background monitor: watch for container exit and update agent status.
                        let state_mon = Arc::clone(&state);
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                let alive = gyre_ports::ComputeTarget::is_alive(&ct, &handle)
                                    .await
                                    .unwrap_or(false);
                                if !alive {
                                    state_mon
                                        .process_registry
                                        .lock()
                                        .await
                                        .remove(&agent_id_str);
                                    // M19.3: Update audit record on container exit.
                                    container_audit::capture_exit_audit(
                                        &state_mon.container_audits,
                                        &agent_id_str,
                                    )
                                    .await;

                                    // M23: Emit container_stopped audit event (best-effort).
                                    {
                                        let exit_code = state_mon
                                            .container_audits
                                            .lock()
                                            .await
                                            .get(&agent_id_str)
                                            .and_then(|r| r.exit_code);
                                        let ctx = crate::container_audit::AuditCtx {
                                            audit: state_mon.audit.as_ref(),
                                            broadcast_tx: &state_mon.audit_broadcast_tx,
                                        };
                                        let container_id_for_evt = state_mon
                                            .container_audits
                                            .lock()
                                            .await
                                            .get(&agent_id_str)
                                            .map(|r| r.container_id.clone())
                                            .unwrap_or_default();
                                        crate::container_audit::emit_stopped(
                                            &ctx,
                                            &agent_id_str,
                                            &container_id_for_evt,
                                            exit_code,
                                        )
                                        .await;
                                    }
                                    if let Ok(Some(mut a)) =
                                        state_mon.agents.find_by_id(&Id::new(&agent_id_str)).await
                                    {
                                        if a.status == AgentStatus::Active {
                                            let _ = a.transition_status(AgentStatus::Idle);
                                            let _ = state_mon.agents.update(&a).await;
                                        }
                                    }
                                    break;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        spawned_pid = None;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        tracing::warn!(
                            agent_id = %agent.id,
                            "container spawn failed (best-effort): {e}. \
                            If the image is missing, build it first: \
                            `docker build -t gyre-agent:latest docker/gyre-agent/`. \
                            Then restart the server with GYRE_AGENT_CREDENTIALS set."
                        );
                    }
                }
            }
            Some(cfg) if cfg.target_type == "ssh" => {
                // M19.5: SSH remote spawn.  When the target config includes
                // `container_mode: true`, wrap the command in a docker run on
                // the remote host using container security defaults (G8).
                let user = cfg.config["user"].as_str().unwrap_or("root").to_string();
                let host = cfg.config["host"]
                    .as_str()
                    .unwrap_or("localhost")
                    .to_string();
                let mut ssh_target = gyre_adapters::compute::SshTarget::new(user, host);
                if let Some(id_file) = cfg.config["identity_file"].as_str() {
                    ssh_target = ssh_target.with_identity(id_file);
                }
                if let Some(port) = cfg.config["port"].as_u64() {
                    ssh_target = ssh_target.with_port(port as u16);
                }

                let container_mode = cfg.config["container_mode"].as_bool().unwrap_or(false);
                let ssh_spawn_config = if container_mode {
                    // M19.5: Build a docker run command to execute on the remote SSH host.
                    // Validate agent name to prevent shell injection (M19.5-A).
                    let safe_name = agent
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
                    if !safe_name || agent.name.is_empty() || agent.name.len() > 63 {
                        return Err(ApiError::InvalidInput(
                            "agent name must be 1-63 chars of [a-zA-Z0-9._-] for container use"
                                .to_string(),
                        ));
                    }
                    let image = cfg.config["image"].as_str().unwrap_or("gyre-agent:latest");
                    // Use direct args (no shell) to prevent injection.
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
                        work_dir: effective_work_dir,
                    }
                } else {
                    spawn_config
                };

                match gyre_ports::ComputeTarget::spawn_process(&ssh_target, &ssh_spawn_config).await
                {
                    Ok(handle) => {
                        spawned_pid = handle.pid;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        let agent_id_str = agent.id.to_string();
                        state
                            .process_registry
                            .lock()
                            .await
                            .insert(agent_id_str.clone(), handle.clone());
                        // Background monitor for SSH.
                        let state_mon = Arc::clone(&state);
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                let alive =
                                    gyre_ports::ComputeTarget::is_alive(&ssh_target, &handle)
                                        .await
                                        .unwrap_or(false);
                                if !alive {
                                    state_mon
                                        .process_registry
                                        .lock()
                                        .await
                                        .remove(&agent_id_str);
                                    if let Ok(Some(mut a)) =
                                        state_mon.agents.find_by_id(&Id::new(&agent_id_str)).await
                                    {
                                        if a.status == AgentStatus::Active {
                                            let _ = a.transition_status(AgentStatus::Idle);
                                            let _ = state_mon.agents.update(&a).await;
                                        }
                                    }
                                    break;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        spawned_pid = None;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        tracing::warn!(agent_id = %agent.id, "SSH spawn failed (best-effort): {e}");
                    }
                }
            }
            _ => {
                // Default: local process spawn.
                let local = gyre_adapters::compute::LocalTarget;
                match gyre_ports::ComputeTarget::spawn_process(&local, &spawn_config).await {
                    Ok(handle) => {
                        spawned_pid = handle.pid;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        let agent_id_str = agent.id.to_string();
                        state
                            .process_registry
                            .lock()
                            .await
                            .insert(agent_id_str.clone(), handle.clone());

                        // Background monitor: watch for process exit and update agent status.
                        let state_mon = Arc::clone(&state);
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                let alive = gyre_ports::ComputeTarget::is_alive(
                                    &gyre_adapters::compute::LocalTarget,
                                    &handle,
                                )
                                .await
                                .unwrap_or(false);
                                if !alive {
                                    state_mon
                                        .process_registry
                                        .lock()
                                        .await
                                        .remove(&agent_id_str);
                                    if let Ok(Some(mut a)) =
                                        state_mon.agents.find_by_id(&Id::new(&agent_id_str)).await
                                    {
                                        if a.status == AgentStatus::Active {
                                            let _ = a.transition_status(AgentStatus::Idle);
                                            let _ = state_mon.agents.update(&a).await;
                                        }
                                    }
                                    break;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        spawned_pid = None;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        tracing::warn!(agent_id = %agent.id, "process spawn failed (best-effort): {e}");
                    }
                }
            }
        }
    }

    // G10 + M19.4: Create workload attestation now that we know the PID / container ID.
    let att = {
        // Retrieve the stack hash recorded by the agent (M14.1), if any.
        let stack_hash = state
            .kv_store
            .kv_get("agent_stacks", &agent.id.to_string())
            .await
            .ok()
            .flatten()
            .and_then(|s| {
                serde_json::from_str::<super::stack_attest::AgentStack>(&s)
                    .ok()
                    .map(|st| st.fingerprint())
            })
            .unwrap_or_default();
        workload_attestation::attest_agent_with_container(
            &agent.id.to_string(),
            spawned_pid,
            compute_target_label,
            &stack_hash,
            spawned_container_id.clone(),
            spawned_container_image.clone(),
        )
    };
    if let Ok(json) = serde_json::to_string(&att) {
        let _ = state
            .kv_store
            .kv_set("workload_attestations", &agent.id.to_string(), json)
            .await;
    }

    // Token was pre-minted above and already stored in agent_tokens.
    // Workload attestation claims are stored in state.workload_attestations
    // and queryable via GET /api/v1/agents/{id}/workload.

    // Auto-track agent spawn
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.spawned",
        Some(agent.id.to_string()),
        serde_json::json!({ "task_id": req.task_id }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    // M22.2: Increment budget active-agent counter for the workspace.
    super::budget::increment_active_agents(&state, &repo.project_id.to_string()).await;

    // M32: Capture meta-spec set SHA for provenance — workspace lookup via kv_store
    // requires a reverse scan (repo_id → workspace_id) which is not directly indexed.
    // Best-effort: omit when workspace cannot be efficiently determined.
    let meta_spec_set_sha: Option<String> = None;

    Ok((
        StatusCode::CREATED,
        Json(SpawnAgentResponse {
            agent: AgentResponse::from(agent),
            token,
            worktree_path,
            clone_url,
            branch: req.branch,
            compute_target_id: req.compute_target_id,
            jj_change_id,
            container_id: spawned_container_id,
            meta_spec_set_sha,
        }),
    ))
}

/// POST /api/v1/agents/{id}/complete
///
/// Agent signals it has finished its task:
/// 1. Creates a MergeRequest (source->target)
/// 2. Transitions task status to Review
/// 3. Transitions agent status to Idle
#[instrument(skip(state, req), fields(agent_id = %id, branch = %req.branch))]
pub async fn complete_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CompleteAgentRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    // Get repository_id from agent's worktree
    let worktrees = state.worktrees.find_by_agent(&agent.id).await?;
    let repo_id = worktrees
        .first()
        .map(|wt| wt.repository_id.clone())
        .ok_or_else(|| ApiError::InvalidInput("agent has no associated worktree".to_string()))?;

    // Idempotent complete: if an MR already exists for this agent+branch, return 202.
    {
        let existing_mrs = state.merge_requests.list_by_repo(&repo_id).await?;
        let found = existing_mrs.into_iter().find(|m| {
            m.source_branch == req.branch
                && m.author_agent_id.as_ref() == Some(&agent.id)
                && m.status != gyre_domain::MrStatus::Closed
        });
        if let Some(existing) = found {
            return Ok((StatusCode::ACCEPTED, Json(MrResponse::from(existing))));
        }
    }

    let now = now_secs();

    // Create MergeRequest
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id,
        req.title,
        req.branch,
        req.target_branch,
        now,
    );
    mr.author_agent_id = Some(agent.id.clone());
    state.merge_requests.create(&mr).await?;

    // Transition task to Review (navigate through intermediate states as needed)
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(mut task)) = state.tasks.find_by_id(task_id).await {
            if task.status == TaskStatus::Backlog {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            if task.status == TaskStatus::Blocked {
                let _ = task.transition_status(TaskStatus::InProgress);
            }
            let _ = task.transition_status(TaskStatus::Review);
            task.updated_at = now;
            let _ = state.tasks.update(&task).await;
        }
    }

    // Transition agent to Idle
    let _ = agent.transition_status(AgentStatus::Idle);
    state.agents.update(&agent).await?;

    // Revoke the agent's token — completed agents must not continue to authenticate (N-1).
    let _ = state.kv_store.kv_remove("agent_tokens", &id).await;

    // Create a jj bookmark for the agent's branch in their worktree (best-effort).
    // This persists the branch tip in jj's bookmark namespace for traceability.
    if let Some(wt) = worktrees.first() {
        if std::path::Path::new(&wt.path).exists() {
            if let Err(e) = state
                .jj_ops
                .jj_bookmark_create(&wt.path, &mr.source_branch, "@")
                .await
            {
                tracing::debug!(agent_id = %agent.id, "jj bookmark skipped: {e}");
            } else {
                tracing::debug!(
                    agent_id = %agent.id,
                    branch = %mr.source_branch,
                    "jj bookmark created on complete"
                );
            }
        }
    }

    // Write snapshot ref for this agent (best-effort)
    if let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await {
        let snap_prefix = format!("refs/agents/{}/snapshots/", agent.id);
        let n = git_refs::count_refs_under(&repo.path, &snap_prefix).await;
        let snap_ref = format!("refs/agents/{}/snapshots/{}", agent.id, n);
        let branch_ref = format!("refs/heads/{}", mr.source_branch);
        if let Some(sha) = git_refs::resolve_ref(&repo.path, &branch_ref).await {
            git_refs::write_ref(&repo.path, &snap_ref, &sha).await;
        }
    }

    // Auto-track agent completion
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.completed",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string() }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    // Auto-track MR creation
    let ev = AnalyticsEvent::new(
        new_id(),
        "mr.created",
        Some(agent.id.to_string()),
        serde_json::json!({ "mr_id": mr.id.to_string(), "source_branch": mr.source_branch }),
        now,
    );
    let _ = state.analytics.record(&ev).await;

    // M22.2: Decrement budget active-agent counter when agent completes.
    if let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await {
        super::budget::decrement_active_agents(&state, &repo.project_id.to_string()).await;
    }

    // Notify the spawning user that an MR needs review (M22.8).
    if let Some(ref spawned_by) = agent.spawned_by {
        crate::notifications::notify_mr_needs_review(
            state.as_ref(),
            spawned_by,
            &mr.id.to_string(),
        )
        .await;
    }

    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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

    async fn create_repo(app: Router) -> (Router, String) {
        let body = serde_json::json!({"project_id": "proj-1", "name": "test-repo"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn create_task(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({"title": title});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    async fn do_spawn(
        app: Router,
        repo_id: &str,
        task_id: &str,
        branch: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({
            "name": "worker-1",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": branch,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;
        (app, json)
    }

    #[tokio::test]
    async fn spawn_creates_agent_active_with_token() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Build feature").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/build").await;

        assert_eq!(json["agent"]["status"], "active");
        assert!(!json["agent"]["id"].as_str().unwrap().is_empty());
        assert!(!json["token"].as_str().unwrap().is_empty());
        assert_eq!(json["branch"], "feat/build");
        // jj_change_id is present in the response (null when worktree doesn't exist on disk)
        assert!(json.get("jj_change_id").is_some());
    }

    #[tokio::test]
    async fn spawn_response_includes_jj_change_id_field() {
        // Verifies the jj_change_id field is present in the spawn response JSON.
        // It will be null if the worktree path doesn't exist on disk (test env),
        // but the field must always be serialized.
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "jj field task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/jj-test").await;

        assert!(
            json.get("jj_change_id").is_some(),
            "spawn response must include jj_change_id field: {json}"
        );
    }

    #[tokio::test]
    async fn spawn_creates_worktree_record() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "WT task").await;
        let (app, _) = do_spawn(app, &repo_id, &task_id, "feat/wt-test").await;

        let wt_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/worktrees"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wt_resp.status(), StatusCode::OK);
        let wt_json = body_json(wt_resp).await;
        assert_eq!(wt_json.as_array().unwrap().len(), 1);
        assert_eq!(wt_json[0]["branch"], "feat/wt-test");
    }

    #[tokio::test]
    async fn spawn_assigns_task_to_agent() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Assigned task").await;
        let (app, json) = do_spawn(app, &repo_id, &task_id, "feat/assign").await;
        let agent_id = json["agent"]["id"].as_str().unwrap().to_string();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["assigned_to"].as_str().unwrap(), &agent_id);
        assert_eq!(task_json["status"], "in_progress");
    }

    #[tokio::test]
    async fn spawn_returns_correct_clone_url_format() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "URL task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/url-test").await;

        let clone_url = json["clone_url"].as_str().unwrap();
        assert!(
            clone_url.contains("/git/"),
            "clone_url should contain /git/: {clone_url}"
        );
        assert!(
            clone_url.contains("proj-1"),
            "clone_url should contain project id: {clone_url}"
        );
        assert!(
            clone_url.contains("test-repo"),
            "clone_url should contain repo name: {clone_url}"
        );
    }

    #[tokio::test]
    async fn spawn_branch_slashes_become_dashes_in_worktree_path() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Slash task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/sub/feature").await;

        let wt_path = json["worktree_path"].as_str().unwrap();
        assert!(
            !wt_path.ends_with("feat/sub/feature"),
            "worktree path should not contain raw branch slashes: {wt_path}"
        );
    }

    #[tokio::test]
    async fn spawn_with_parent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Child task").await;

        let body = serde_json::json!({
            "name": "child-worker",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/child",
            "parent_id": "parent-agent-123",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["agent"]["parent_id"], "parent-agent-123");
    }

    #[tokio::test]
    async fn spawn_requires_valid_auth_token() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_bad_token_rejected() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "r",
            "task_id": "t",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer bad-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn spawn_repo_not_found() {
        let app = app();
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": "no-such-repo",
            "task_id": "task-1",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn spawn_task_not_found() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let body = serde_json::json!({
            "name": "worker",
            "repo_id": repo_id,
            "task_id": "no-such-task",
            "branch": "feat/x",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_creates_merge_request() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Feature task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/complete-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/complete-test",
            "title": "Add my feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["source_branch"], "feat/complete-test");
        assert_eq!(mr_json["target_branch"], "main");
        assert_eq!(mr_json["title"], "Add my feature");
        assert_eq!(mr_json["status"], "open");
    }

    #[tokio::test]
    async fn complete_transitions_task_to_review() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Review task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/review-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/review-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let task_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let task_json = body_json(task_resp).await;
        assert_eq!(task_json["status"], "review");
    }

    #[tokio::test]
    async fn complete_transitions_agent_to_idle() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Idle task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/idle-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/idle-test",
            "title": "Done",
            "target_branch": "main",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let agent_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{agent_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let agent_json = body_json(agent_resp).await;
        assert_eq!(agent_json["status"], "idle");
    }

    #[tokio::test]
    async fn complete_agent_not_found() {
        let body = serde_json::json!({
            "branch": "feat/x",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/no-such/complete")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_no_worktree_returns_bad_request() {
        let app = app();
        // Create agent directly (no worktree)
        let body = serde_json::json!({"name": "bare-agent"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let agent_id = json["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "main",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn complete_mr_sets_author_agent_id() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Author task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/author-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/author-test",
            "title": "Feature",
            "target_branch": "main",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        assert_eq!(mr_json["author_agent_id"].as_str().unwrap(), &agent_id);
    }

    #[tokio::test]
    async fn complete_revokes_agent_token() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Revoke task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/revoke-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();
        let agent_token = spawn_json["token"].as_str().unwrap().to_string();

        // Complete the agent
        let body = serde_json::json!({
            "branch": "feat/revoke-test",
            "title": "Done",
            "target_branch": "main",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // The agent's token must now be rejected (401) — it was revoked on complete.
        let spawn_body = serde_json::json!({
            "name": "should-fail",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/should-fail",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/spawn")
                    .header("content-type", "application/json")
                    .header("Authorization", format!("Bearer {agent_token}"))
                    .body(Body::from(serde_json::to_vec(&spawn_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "agent token must be invalid after complete"
        );
    }

    #[tokio::test]
    async fn complete_idempotent_returns_202_on_double_complete() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Idempotent task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/idempotent-test").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/idempotent-test",
            "title": "Idempotent Feature",
            "target_branch": "main",
        });

        // First complete — should return 201 CREATED
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::CREATED);
        let mr_json1 = body_json(resp1).await;
        let mr_id = mr_json1["id"].as_str().unwrap().to_string();

        // Second complete — should return 202 ACCEPTED with the same MR id
        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::ACCEPTED);
        let mr_json2 = body_json(resp2).await;
        assert_eq!(mr_json2["id"].as_str().unwrap(), &mr_id);
    }
}
