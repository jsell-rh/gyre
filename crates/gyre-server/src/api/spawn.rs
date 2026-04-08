use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    policy::{Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope},
    Agent, AgentStatus, AgentUsage, AgentWorktree, AnalyticsEvent, ComputeTargetEntity,
    ComputeTargetType, DisconnectedBehavior, LoopConfig, MergeRequest, TaskStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::{auth::AuthenticatedAgent, container_audit, git_refs, workload_attestation, AppState};

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
    /// When present, the server manages the Ralph loop session cycle automatically.
    /// When absent, the agent runs a single session (backward-compatible).
    pub loop_config: Option<LoopConfig>,
    /// Agent type: null for normal agents, "interrogation" for interrogation agents (HSI §4).
    pub agent_type: Option<String>,
    /// For interrogation agents: SHA-256 of the original agent's conversation to load as context.
    pub conversation_sha: Option<String>,
}

/// JWT TTL for interrogation agents: 30 minutes (HSI §4).
const INTERROGATION_JWT_TTL_SECS: u64 = 1800;

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
    /// SHA-256 of the agent's conversation blob (HSI §5 provenance).
    /// Stored in KV so the merge attestation can include it.
    #[serde(default)]
    pub conversation_sha: Option<String>,
}

// ── Interrogation agent helpers ───────────────────────────────────────────────

/// Create the three ABAC policies required for an interrogation agent (HSI §4).
/// Returns the list of created policy IDs (stored in kv_store for cleanup).
pub async fn create_interrogation_policies(state: &AppState, agent_id: &str) -> Vec<String> {
    let now = now_secs();
    let subject_value = format!("agent:{agent_id}");

    let restrict_id = format!("interrogation-restrict-{agent_id}");
    let allow_message_id = format!("interrogation-allow-message-{agent_id}");
    let allow_read_id = format!("interrogation-allow-read-{agent_id}");

    let policies: Vec<Policy> = vec![
        // Deny write/delete/spawn/approve/merge on all non-message resources (priority 200).
        Policy {
            id: Id::new(&restrict_id),
            name: restrict_id.clone(),
            description: format!(
                "Interrogation agent {agent_id} is read-only + message to requesting human"
            ),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 200,
            effect: PolicyEffect::Deny,
            conditions: vec![Condition {
                attribute: "subject.id".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String(subject_value.clone()),
            }],
            actions: vec![
                "write".to_string(),
                "delete".to_string(),
                "spawn".to_string(),
                "approve".to_string(),
                "merge".to_string(),
            ],
            resource_types: vec![
                "task".to_string(),
                "mr".to_string(),
                "repo".to_string(),
                "agent".to_string(),
                "spec".to_string(),
                "persona".to_string(),
                "worktree".to_string(),
            ],
            enabled: true,
            immutable: false,
            built_in: false,
            created_by: "system".to_string(),
            created_at: now,
            updated_at: now,
        },
        // Allow write to message resource (priority 201).
        Policy {
            id: Id::new(&allow_message_id),
            name: allow_message_id.clone(),
            description: format!("Interrogation agent {agent_id} can send messages"),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 201,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.id".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String(subject_value.clone()),
            }],
            actions: vec!["write".to_string()],
            resource_types: vec!["message".to_string()],
            enabled: true,
            immutable: false,
            built_in: false,
            created_by: "system".to_string(),
            created_at: now,
            updated_at: now,
        },
        // Allow read to conversation/explorer_view/spec/mr/repo/task (priority 202).
        Policy {
            id: Id::new(&allow_read_id),
            name: allow_read_id.clone(),
            description: format!("Interrogation agent {agent_id} can read context resources"),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 202,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.id".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String(subject_value),
            }],
            actions: vec!["read".to_string()],
            resource_types: vec![
                "conversation".to_string(),
                "explorer_view".to_string(),
                "spec".to_string(),
                "mr".to_string(),
                "repo".to_string(),
                "task".to_string(),
            ],
            enabled: true,
            immutable: false,
            built_in: false,
            created_by: "system".to_string(),
            created_at: now,
            updated_at: now,
        },
    ];

    let mut created_ids = Vec::new();
    for policy in &policies {
        match state.policies.create(policy).await {
            Ok(()) => created_ids.push(policy.id.to_string()),
            Err(e) => tracing::warn!(
                agent_id = %agent_id,
                policy_id = %policy.id,
                "failed to create interrogation policy: {e}"
            ),
        }
    }

    // Store policy IDs in kv_store for cleanup on complete/kill/stale.
    if let Ok(ids_json) = serde_json::to_string(&created_ids) {
        let _ = state
            .kv_store
            .kv_set("interrogation_policies", agent_id, ids_json)
            .await;
    }

    created_ids
}

/// Delete all ABAC policies created for an interrogation agent.
/// Called on agent.complete, admin kill, and stale agent detection.
pub async fn cleanup_interrogation_policies(state: &AppState, agent_id: &str) {
    let ids_json = match state
        .kv_store
        .kv_get("interrogation_policies", agent_id)
        .await
        .ok()
        .flatten()
    {
        Some(j) => j,
        None => return,
    };

    let ids: Vec<String> = match serde_json::from_str(&ids_json) {
        Ok(v) => v,
        Err(_) => return,
    };

    for id in &ids {
        if let Err(e) = state.policies.delete(id).await {
            tracing::warn!(
                agent_id = %agent_id,
                policy_id = %id,
                "failed to delete interrogation policy: {e}"
            );
        }
    }

    // Remove the kv entry.
    let _ = state
        .kv_store
        .kv_remove("interrogation_policies", agent_id)
        .await;

    tracing::info!(
        agent_id = %agent_id,
        count = ids.len(),
        "interrogation policies cleaned up"
    );
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
    super::budget::check_spawn_budget(&state, &repo.workspace_id.to_string())
        .await
        .map_err(ApiError::TooManyRequests)?;

    // Verify task exists
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&req.task_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {} not found", req.task_id)))?;

    // Agent-runtime §1 Phase 4: Only Implementation tasks trigger worker agent spawning.
    // Delegation and Coordination tasks trigger orchestrator spawning (different path).
    // Tasks without a task_type (pre-approval push-hook tasks) do NOT trigger agent spawning.
    // Exception: interrogation agents bypass this check (they are system-initiated, not task-driven).
    let is_interrogation = req.agent_type.as_deref() == Some("interrogation");
    if !is_interrogation {
        match &task.task_type {
            Some(gyre_domain::TaskType::Implementation) => { /* allowed */ }
            Some(gyre_domain::TaskType::Delegation) => {
                return Err(ApiError::InvalidInput(
                    "delegation tasks trigger orchestrator spawning, not worker agent spawning"
                        .to_string(),
                ));
            }
            Some(gyre_domain::TaskType::Coordination) => {
                return Err(ApiError::InvalidInput(
                    "coordination tasks trigger orchestrator spawning, not worker agent spawning"
                        .to_string(),
                ));
            }
            None => {
                return Err(ApiError::InvalidInput(
                    "tasks without a task_type (pre-approval push-hook tasks) cannot trigger agent spawning; set task_type to 'implementation' first"
                        .to_string(),
                ));
            }
        }
    }

    // Fetch workspace for compute target resolution and clone URL.
    let workspace = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await
        .ok()
        .flatten();

    // Resolve compute target via DB: request ID → workspace assignment → tenant default.
    // If req.compute_target_id is set it must exist; return 404 immediately if not found.
    let resolved_ct_entity: Option<ComputeTargetEntity> =
        if let Some(ref ct_id) = req.compute_target_id {
            let entity = state
                .compute_targets
                .get_by_id(&Id::new(ct_id))
                .await
                .ok()
                .flatten();
            if entity.is_none() {
                return Err(ApiError::NotFound(format!(
                    "compute target {ct_id} not found"
                )));
            }
            entity
        } else if let Some(ws_ct_id) = workspace
            .as_ref()
            .and_then(|ws| ws.compute_target_id.clone())
        {
            // Workspace-assigned compute target.
            state
                .compute_targets
                .get_by_id(&ws_ct_id)
                .await
                .ok()
                .flatten()
        } else if let Some(ref ws) = workspace {
            // Tenant default compute target.
            state
                .compute_targets
                .get_default_for_tenant(&ws.tenant_id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };

    let now = now_secs();

    // Reject duplicate agent names with a clear 400 rather than a 500.
    if let Ok(Some(_)) = state.agents.find_by_name(&req.name).await {
        return Err(ApiError::InvalidInput(format!(
            "an agent named '{}' already exists; choose a different name",
            req.name
        )));
    }

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

    // HSI §4: Interrogation agents get a short-lived 30-minute JWT.
    let jwt_ttl = if is_interrogation {
        INTERROGATION_JWT_TTL_SECS
    } else {
        state.agent_jwt_ttl_secs
    };

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
            jwt_ttl,
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

    // HSI §4: For interrogation agents — create scoped ABAC policies and store
    // the conversation context for the conversation://context MCP resource.
    if is_interrogation {
        create_interrogation_policies(&state, &agent.id.to_string()).await;

        // Retrieve conversation from kv_store (written by S2.1 ConversationRepository).
        // Best-effort: if not found, the MCP resource will return an empty context.
        if let Some(sha) = &req.conversation_sha {
            if let Ok(Some(blob)) = state.kv_store.kv_get("conversations", sha.as_str()).await {
                let _ = state
                    .kv_store
                    .kv_set("interrogation_context", &agent.id.to_string(), blob)
                    .await;
            }
        }
    }

    // Compute worktree path: {repo_path}/worktrees/{branch_slug}
    let branch_slug = req.branch.replace('/', "-");
    let worktree_path = format!("{}/worktrees/{}", repo.path, branch_slug);

    // HSI §4: Interrogation agents are read-only — they have no worktree.
    // Skip worktree creation, jj init, and git ref writes.
    let jj_change_id = if !is_interrogation {
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
        let change_id = if std::path::Path::new(&worktree_path).exists() {
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
            let task_ref = format!("refs/tasks/{}", task.id);
            git_refs::write_ref(&repo.path, &agent_ref, &sha).await;
            git_refs::write_ref(&repo.path, &task_ref, &sha).await;
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

        change_id
    } else {
        None
    };

    // Assign task to agent and advance to InProgress
    task.assigned_to = Some(agent.id.clone());
    if task.status == TaskStatus::Backlog {
        let _ = task.transition_status(TaskStatus::InProgress);
    }
    task.updated_at = now;
    state.tasks.update(&task).await?;

    // Build clone URL: {base_url}/git/{workspace_slug}/{repo_name}
    // Reuse the already-fetched workspace; fall back to workspace_id if not found.
    let ws_slug = workspace
        .as_ref()
        .map(|ws| ws.slug.clone())
        .unwrap_or_else(|| repo.workspace_id.to_string());
    let clone_url = format!("{}/git/{}/{}", state.base_url, ws_slug, repo.name);

    // M19.1: Resolve the effective compute target.
    // Priority: request compute_target_id → workspace assignment → tenant default → local.
    //
    // resolved_ct_entity was computed earlier from the DB; convert it to ComputeTargetConfig
    // for the existing spawn dispatch logic below.
    let compute_target_label = resolved_ct_entity
        .as_ref()
        .map(|e| e.id.to_string())
        .unwrap_or_else(|| "local".to_string());

    let resolved_target_config: Option<super::compute::ComputeTargetConfig> = resolved_ct_entity
        .as_ref()
        .map(|e| super::compute::ComputeTargetConfig {
            id: e.id.to_string(),
            name: e.name.clone(),
            target_type: match e.target_type {
                ComputeTargetType::Container => "container".to_string(),
                ComputeTargetType::Ssh => "ssh".to_string(),
                ComputeTargetType::Kubernetes => "kubernetes".to_string(),
            },
            config: e.config.clone(),
        });

    // Launch a real process and monitor its lifecycle.
    // Capture the PID (local) or container ID (container) for workload attestation.
    let spawned_pid: Option<u32>;
    let spawned_container_id: Option<String>; // M19.1/M19.3/M19.4
    let spawned_container_image: Option<String>; // M19.3/M19.4

    {
        // Docker requires an absolute working directory path. Canonicalize the
        // worktree path to ensure it's absolute even when GYRE_REPOS_PATH is
        // relative (e.g. the default "./repos/").
        let effective_work_dir = if std::path::Path::new(&worktree_path).exists() {
            std::fs::canonicalize(&worktree_path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| worktree_path.clone())
        } else {
            // Worktree not yet on disk — fall back to /workspace (absolute).
            "/workspace".to_string()
        };
        // Command is server-controlled only — never from user input (C-1 RCE fix).
        // Use compute target's configured command, GYRE_AGENT_COMMAND env var, or /gyre/entrypoint.sh.
        let command = resolved_target_config
            .as_ref()
            .and_then(|cfg| cfg.config.get("command"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| std::env::var("GYRE_AGENT_COMMAND").ok())
            .unwrap_or_else(|| "/gyre/entrypoint.sh".to_string());
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

        // Vertex AI: forward non-secret Vertex config env vars to the container.
        // Secrets (GCP SA JSON) are handled by GYRE_CRED_GCP_SA_JSON above.
        // CLAUDE_CODE_USE_VERTEX enables Vertex mode in the Claude Agent SDK.
        for var_name in [
            "CLAUDE_CODE_USE_VERTEX",
            "ANTHROPIC_VERTEX_PROJECT_ID",
            "CLOUD_ML_REGION",
        ] {
            if let Ok(val) = std::env::var(var_name) {
                if !val.is_empty() {
                    container_env.insert(var_name.to_string(), val);
                }
            }
        }
        // Also check GYRE_VERTEX_LOCATION as an alias for CLOUD_ML_REGION.
        if !container_env.contains_key("CLOUD_ML_REGION") {
            if let Ok(val) = std::env::var("GYRE_VERTEX_LOCATION") {
                if !val.is_empty() {
                    container_env.insert("CLOUD_ML_REGION".to_string(), val);
                }
            }
        }

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
                        let _ = state.container_audits.save(&rec).await;

                        // M19.3: Emit AgentContainerSpawned event.
                        state
                            .emit_event(
                                Some(agent.workspace_id.clone()),
                                gyre_common::message::Destination::Workspace(
                                    agent.workspace_id.clone(),
                                ),
                                gyre_common::message::MessageKind::AgentContainerSpawned,
                                Some(serde_json::json!({
                                    "agent_id": agent.id.to_string(),
                                    "container_id": handle.id,
                                    "image": image,
                                    "runtime": runtime_str,
                                })),
                            )
                            .await;

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
                                        state_mon.container_audits.as_ref(),
                                        &agent_id_str,
                                    )
                                    .await;

                                    // M23: Emit container_stopped audit event (best-effort).
                                    {
                                        let audit_rec = state_mon
                                            .container_audits
                                            .find_by_agent_id(&agent_id_str)
                                            .await
                                            .ok()
                                            .flatten();
                                        let exit_code =
                                            audit_rec.as_ref().and_then(|r| r.exit_code);
                                        let ctx = crate::container_audit::AuditCtx {
                                            audit: state_mon.audit.as_ref(),
                                            broadcast_tx: &state_mon.audit_broadcast_tx,
                                        };
                                        let container_id_for_evt =
                                            audit_rec.map(|r| r.container_id).unwrap_or_default();
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
            &compute_target_label,
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

    // Phase 3 (TASK-008, §7.4): Create workload KeyBinding and DerivedInput
    // from the parent task's attestation chain, then inject into the agent's
    // environment via KV store. The agent uses the KeyBinding to sign its
    // output attestation at push time.
    if !is_interrogation {
        create_derived_input_for_agent(
            &state,
            &agent.id.to_string(),
            &req.task_id,
            &auth.agent_id,
            now,
        )
        .await;
    }

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
    super::budget::increment_active_agents(&state, &repo.workspace_id.to_string()).await;

    // M32: Capture meta-spec set SHA for provenance — workspace lookup via kv_store
    // requires a reverse scan (repo_id → workspace_id) which is not directly indexed.
    // Best-effort: omit when workspace cannot be efficiently determined.
    let meta_spec_set_sha: Option<String> = None;

    Ok((
        StatusCode::CREATED,
        Json(SpawnAgentResponse {
            agent: {
                let mut r = AgentResponse::from(agent);
                r.repo_id = Some(req.repo_id.clone());
                r.branch = Some(req.branch.clone());
                r.task_id = Some(req.task_id.clone());
                r
            },
            token,
            worktree_path,
            clone_url,
            branch: req.branch,
            compute_target_id: resolved_ct_entity.as_ref().map(|e| e.id.to_string()),
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

    // Propagate spec_ref from the task to the MR for provenance linkage.
    // If the task has a spec_path, build a spec_ref in "path@sha" format
    // by looking up the current SHA from the spec ledger.
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(task)) = state.tasks.find_by_id(task_id).await {
            if let Some(ref spec_path) = task.spec_path {
                if let Ok(Some(entry)) = state.spec_ledger.find_by_path(spec_path).await {
                    mr.spec_ref = Some(format!("{}@{}", spec_path, entry.current_sha));
                } else {
                    // Spec not in ledger — use path without SHA.
                    mr.spec_ref = Some(spec_path.clone());
                }
            }
        }
    }

    // Compute diff stats + conflict detection (like create_mr does).
    if let Ok(Some(repo)) = state.repos.find_by_id(&mr.repository_id).await {
        mr.workspace_id = repo.workspace_id.clone();
        if let Ok(diff) = state
            .git_ops
            .diff(&repo.path, &mr.target_branch, &mr.source_branch)
            .await
        {
            mr.diff_stats = Some(gyre_domain::DiffStats {
                files_changed: diff.files_changed,
                insertions: diff.insertions,
                deletions: diff.deletions,
            });
        }
        if let Ok(can_merge) = state
            .git_ops
            .can_merge(&repo.path, &mr.source_branch, &mr.target_branch)
            .await
        {
            mr.has_conflicts = Some(!can_merge);
        }
    }

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

    // Store conversation SHA in KV for merge attestation (HSI §5).
    // The merge processor looks up `conv_sha:{agent_id}` in the `agent_provenance` bucket.
    if let Some(ref sha) = req.conversation_sha {
        let kv_key = format!("conv_sha:{}", id);
        let _ = state
            .kv_store
            .kv_set("agent_provenance", &kv_key, sha.clone())
            .await;
    }

    // Transition agent to Idle
    let _ = agent.transition_status(AgentStatus::Idle);
    state.agents.update(&agent).await?;

    // Revoke the agent's token — completed agents must not continue to authenticate (N-1).
    let _ = state.kv_store.kv_remove("agent_tokens", &id).await;

    // HSI §4: Clean up interrogation ABAC policies on completion.
    cleanup_interrogation_policies(&state, &id).await;
    // Also remove any stored conversation context for this agent.
    let _ = state.kv_store.kv_remove("interrogation_context", &id).await;

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

    // Write snapshot ref for this agent (best-effort — never fail the complete request)
    let snap_result: Result<(), anyhow::Error> = async {
        let repo = state
            .repos
            .find_by_id(&mr.repository_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("repo not found"))?;
        let snap_prefix = format!("refs/agents/{}/snapshots/", agent.id);
        let n = git_refs::count_refs_under(&repo.path, &snap_prefix).await;
        let snap_ref = format!("refs/agents/{}/snapshots/{}", agent.id, n);
        let branch_ref = format!("refs/heads/{}", mr.source_branch);
        if let Some(sha) = git_refs::resolve_ref(&repo.path, &branch_ref).await {
            git_refs::write_ref(&repo.path, &snap_ref, &sha).await;
        }
        Ok(())
    }
    .await;
    if let Err(e) = snap_result {
        tracing::warn!(
            agent_id = %agent.id,
            error = %e,
            "snapshot ref write failed (best-effort, ignoring)"
        );
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
        super::budget::decrement_active_agents(&state, &repo.workspace_id.to_string()).await;
    }

    // Notify the spawning user that the agent completed and an MR is ready for review (HSI §2).
    if let Some(ref spawned_by) = agent.spawned_by {
        let body_json = serde_json::json!({
            "agent_id": agent.id.as_str(),
            "agent_name": &agent.name,
            "mr_id": mr.id.as_str(),
            "mr_title": &mr.title,
            "spec_path": mr.spec_ref.as_ref().map(|s| s.split('@').next().unwrap_or(s)),
        })
        .to_string();

        crate::notifications::notify_rich(
            state.as_ref(),
            mr.workspace_id.clone(),
            Id::new(spawned_by.clone()),
            gyre_common::NotificationType::AgentCompleted,
            format!(
                "Agent '{}' completed — MR '{}' is ready for review",
                agent.name, mr.title
            ),
            "default",
            Some(body_json),
            Some(mr.id.to_string()),
            Some(mr.repository_id.to_string()),
        )
        .await;
    }

    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

// ── Agent lifecycle endpoints ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RecordUsageRequest {
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub cost_usd: f64,
}

/// POST /api/v1/agents/:id/usage
///
/// Agent reports its token/cost usage for the current session.
/// Auth: agent-scoped JWT — the reporting agent must match :id.
/// Returns 200 OK, or 429 if workspace budget is exhausted (best-effort check).
#[instrument(skip(state), fields(agent_id = %id))]
pub async fn record_agent_usage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RecordUsageRequest>,
) -> Result<StatusCode, ApiError> {
    let agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    let now = now_secs();
    let usage = AgentUsage {
        agent_id: agent.id.clone(),
        tokens_input: req.tokens_input,
        tokens_output: req.tokens_output,
        cost_usd: req.cost_usd,
        reported_at: now,
    };

    state.agents.record_usage(&usage).await?;

    tracing::info!(
        agent_id = %id,
        tokens_input = req.tokens_input,
        tokens_output = req.tokens_output,
        cost_usd = req.cost_usd,
        "agent usage recorded"
    );

    Ok(StatusCode::OK)
}

/// POST /api/v1/agents/:id/fail
///
/// Marks an agent as Failed (non-recoverable error). Idempotent.
#[instrument(skip(state), fields(agent_id = %id))]
pub async fn fail_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    if agent.status == AgentStatus::Failed {
        return Ok(StatusCode::OK);
    }

    agent
        .transition_status(AgentStatus::Failed)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.update(&agent).await?;

    // M22.2: Decrement budget active-agent counter.
    let workspace_id = agent.workspace_id.to_string();
    super::budget::decrement_active_agents(&state, &workspace_id).await;

    // Notify the spawning user that the agent failed (HSI §2).
    if let Some(ref spawned_by) = agent.spawned_by {
        crate::notifications::notify(
            state.as_ref(),
            agent.workspace_id.clone(),
            Id::new(spawned_by.clone()),
            gyre_common::NotificationType::AgentEscalation,
            format!("Agent '{}' failed and needs attention", agent.name),
            "default",
        )
        .await;
    }

    Ok(StatusCode::OK)
}

/// POST /api/v1/agents/:id/stop
///
/// Marks an agent as Stopped (operator/orchestrator initiated). Idempotent.
#[instrument(skip(state), fields(agent_id = %id))]
pub async fn stop_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut agent = state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    if agent.status == AgentStatus::Stopped {
        return Ok(StatusCode::OK);
    }

    agent
        .transition_status(AgentStatus::Stopped)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.update(&agent).await?;

    // Revoke the agent's token so it can no longer authenticate.
    let _ = state.kv_store.kv_remove("agent_tokens", &id).await;

    // M22.2: Decrement budget active-agent counter.
    let workspace_id = agent.workspace_id.to_string();
    super::budget::decrement_active_agents(&state, &workspace_id).await;

    Ok(StatusCode::OK)
}

// ── Derived Input (Phase 3, §7.4) ─────────────────────────────────────────────

/// Create a workload `KeyBinding` and `DerivedInput` for a newly spawned agent
/// from the parent task's attestation chain. Stored in KV so the agent can use
/// the key to sign its output attestation at push time.
///
/// Best-effort: if no attestation chain exists for the task, skips silently
/// (the agent may be for a task that predates the provenance system).
///
/// This function CREATES (generates + signs) crypto material — it does not
/// consume/verify externally-submitted crypto material.
async fn create_derived_input_for_agent(
    state: &crate::AppState,
    agent_id: &str,
    task_id: &str,
    spawner_id: &str,
    now: u64,
) {
    // crypto-verify:ok — this function generates+signs new keys, not verifying external input.
    // Look up parent attestation chain for this task.
    let attestations = match state.chain_attestations.find_by_task(task_id).await {
        Ok(atts) if !atts.is_empty() => atts,
        _ => {
            tracing::debug!(
                agent_id = %agent_id,
                task_id = %task_id,
                "no attestation chain for task — skipping DerivedInput creation"
            );
            return;
        }
    };

    // Find the leaf attestation (most recent) to derive from.
    let parent_att = match attestations.last() {
        Some(a) => a,
        None => return,
    };

    // Chain depth limit check (§4.6): hard limit 10, configurable per workspace.
    if parent_att.metadata.chain_depth >= 10 {
        tracing::warn!(
            agent_id = %agent_id,
            task_id = %task_id,
            chain_depth = parent_att.metadata.chain_depth,
            "chain depth limit reached (max 10) — skipping DerivedInput creation"
        );
        return;
    }

    // ── Step 1: Load the SPAWNER's (orchestrator's) signing key (§4.1, §4.5) ──
    // The DerivedInput must be signed by the spawner (parent/orchestrator), NOT
    // the child agent being spawned. This proves that a specific orchestrator
    // authorized the delegation. The spawner's key was stored when the spawner
    // itself was spawned (or when the orchestrator first received its key).
    let spawner_key_b64 = match state
        .kv_store
        .kv_get("agent_signing_keys", spawner_id)
        .await
    {
        Ok(Some(k)) => k,
        _ => {
            tracing::warn!(
                agent_id = %agent_id,
                spawner_id = %spawner_id,
                "spawner has no signing key in KV — skipping DerivedInput creation"
            );
            return;
        }
    };
    let spawner_pkcs8 = match base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &spawner_key_b64,
    ) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(spawner_id = %spawner_id, "failed to decode spawner key: {e}");
            return;
        }
    };
    let spawner_key_pair = match ring::signature::Ed25519KeyPair::from_pkcs8(&spawner_pkcs8) {
        Ok(kp) => kp,
        Err(e) => {
            tracing::warn!(spawner_id = %spawner_id, "failed to parse spawner keypair: {e}");
            return;
        }
    };

    // Load the spawner's own KeyBinding. Check `agent_key_bindings` first (the
    // canonical location), then fall back to extracting from `agent_derived_inputs`
    // (backward compatibility). If neither exists, the spawner is the root agent —
    // build a KeyBinding from its public key.
    use ring::signature::KeyPair;
    let spawner_kb = {
        // Try the canonical key binding store first.
        let from_kb_store = state
            .kv_store
            .kv_get("agent_key_bindings", spawner_id)
            .await
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<gyre_common::KeyBinding>(&json).ok());
        if let Some(kb) = from_kb_store {
            kb
        } else {
            // Fall back to extracting from the spawner's DerivedInput (backward compat).
            let from_di = state
                .kv_store
                .kv_get("agent_derived_inputs", spawner_id)
                .await
                .ok()
                .flatten()
                .and_then(|json| {
                    serde_json::from_str::<gyre_common::DerivedInput>(&json)
                        .ok()
                        .map(|di| di.key_binding)
                });
            match from_di {
                Some(kb) => kb,
                None => {
                    // Spawner is the root agent — build a KeyBinding from its public key.
                    let spawner_pub = spawner_key_pair.public_key().as_ref().to_vec();
                    gyre_common::KeyBinding {
                        public_key: spawner_pub,
                        user_identity: format!("agent:{spawner_id}"),
                        issuer: state.base_url.clone(),
                        trust_anchor_id: "gyre-oidc".to_string(),
                        issued_at: now,
                        expires_at: now + state.agent_jwt_ttl_secs,
                        user_signature: vec![],
                        platform_countersign: vec![],
                    }
                }
            }
        }
    };

    // ── Step 2: Generate the CHILD agent's keypair for its own future use ──
    // The child agent needs its own keypair to sign output attestations at push
    // time. This keypair is stored in KV for the child, separate from the
    // spawner's key used to sign the DerivedInput.
    let rng = ring::rand::SystemRandom::new();
    let child_pkcs8 = match ring::signature::Ed25519KeyPair::generate_pkcs8(&rng) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(agent_id = %agent_id, "failed to generate agent keypair: {e}");
            return;
        }
    };
    let child_key_pair = match ring::signature::Ed25519KeyPair::from_pkcs8(child_pkcs8.as_ref()) {
        Ok(kp) => kp,
        Err(e) => {
            tracing::warn!(agent_id = %agent_id, "failed to parse agent keypair: {e}");
            return;
        }
    };
    let child_public_key = child_key_pair.public_key().as_ref().to_vec();

    // Build the child agent's own workload KeyBinding (for push-time signing).
    let child_kb = gyre_common::KeyBinding {
        public_key: child_public_key,
        user_identity: format!("agent:{agent_id}"),
        issuer: state.base_url.clone(),
        trust_anchor_id: "gyre-oidc".to_string(),
        issued_at: now,
        expires_at: now + state.agent_jwt_ttl_secs,
        user_signature: vec![],       // workload-bound — no user signature
        platform_countersign: vec![], // placeholder:ok — workload-bound key bindings use agent key, not platform countersign
    };

    // ── Step 3: Sign the DerivedInput with the SPAWNER's key (§4.1, §4.5) ──
    // Compute parent_ref as content hash of parent attestation.
    let parent_bytes = serde_json::to_vec(parent_att).unwrap_or_default();
    let parent_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&parent_bytes);
        hasher.finalize().to_vec()
    };

    // Sign the derivation with the spawner's (orchestrator's) key.
    let derivation_content = serde_json::json!({
        "parent_ref": hex::encode(&parent_hash),
        "agent_id": agent_id,
        "task_id": task_id,
    });
    let derivation_bytes = serde_json::to_vec(&derivation_content).unwrap_or_default();
    let content_hash = {
        use ring::digest;
        digest::digest(&digest::SHA256, &derivation_bytes)
    };
    let sig = spawner_key_pair
        .sign(content_hash.as_ref())
        .as_ref()
        .to_vec();

    // Build the DerivedInput (§4.1) — signed by the spawner, with the spawner's
    // KeyBinding. This proves the orchestrator authorized this delegation.
    let derived_input = gyre_common::DerivedInput {
        parent_ref: parent_hash,
        preconditions: vec![],
        update: format!("agent:{agent_id} spawned for task:{task_id}"),
        output_constraints: vec![], // inherited from parent (additive only)
        signature: sig,
        key_binding: spawner_kb.clone(),
    };

    // Store the DerivedInput in KV so the child agent can retrieve it.
    if let Ok(di_json) = serde_json::to_string(&derived_input) {
        let _ = state
            .kv_store
            .kv_set("agent_derived_inputs", agent_id, di_json)
            .await;
    }

    // Store the child agent's own private key so it can sign output attestations.
    let _ = state
        .kv_store
        .kv_set(
            "agent_signing_keys",
            agent_id,
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                child_pkcs8.as_ref(),
            ),
        )
        .await;

    // Store the child agent's own KeyBinding separately so that when this agent
    // later acts as a spawner, its KeyBinding can be attached to the DerivedInput
    // it signs. Uses a separate namespace to avoid overwriting the actual
    // DerivedInput stored above (which carries the spawner's KeyBinding).
    if let Ok(kb_json) = serde_json::to_string(&child_kb) {
        let _ = state
            .kv_store
            .kv_set("agent_key_bindings", agent_id, kb_json)
            .await;
    }

    // Create an attestation record for this derivation.
    let new_att = gyre_common::Attestation {
        id: uuid::Uuid::new_v4().to_string(),
        input: gyre_common::AttestationInput::Derived(derived_input),
        output: gyre_common::AttestationOutput {
            content_hash: vec![],
            commit_sha: String::new(),
            agent_signature: None,
            gate_results: vec![],
        },
        metadata: gyre_common::AttestationMetadata {
            created_at: now,
            workspace_id: parent_att.metadata.workspace_id.clone(),
            repo_id: parent_att.metadata.repo_id.clone(),
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            chain_depth: parent_att.metadata.chain_depth + 1,
        },
    };

    if let Err(e) = state.chain_attestations.save(&new_att).await {
        tracing::warn!(
            agent_id = %agent_id,
            error = %e,
            "failed to save derived attestation for agent"
        );
    } else {
        // §7.7: attestation.created audit event for derived input.
        tracing::info!(
            agent_id = %agent_id,
            task_id = %task_id,
            spawner = %spawner_id,
            chain_depth = new_att.metadata.chain_depth,
            category = "Provenance",
            event = "attestation.created",
            "attestation.created: DerivedInput signed by spawner {spawner_id} for agent {agent_id} (depth {})",
            new_att.metadata.chain_depth
        );
    }
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
        let body = serde_json::json!({"workspace_id": "ws-1", "name": "test-repo"});
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
        let body = serde_json::json!({"title": title, "task_type": "implementation"});
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
            clone_url.contains("ws-1"),
            "clone_url should contain workspace slug/id: {clone_url}"
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

    // ── Interrogation agent tests (HSI §4) ────────────────────────────────────

    async fn do_spawn_interrogation(
        app: Router,
        repo_id: &str,
        task_id: &str,
        conversation_sha: Option<&str>,
    ) -> (Router, serde_json::Value) {
        let mut body = serde_json::json!({
            "name": "interrogation-1",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "interrogation/test",
            "agent_type": "interrogation",
        });
        if let Some(sha) = conversation_sha {
            body["conversation_sha"] = serde_json::Value::String(sha.to_string());
        }
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
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "interrogation spawn should succeed"
        );
        let json = body_json(resp).await;
        (app, json)
    }

    #[tokio::test]
    async fn interrogation_spawn_creates_active_agent() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Interrogate agent").await;
        let (_, json) = do_spawn_interrogation(app, &repo_id, &task_id, None).await;

        assert_eq!(json["agent"]["status"], "active");
        assert!(!json["token"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn interrogation_spawn_creates_abac_policies() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "Policy agent").await;
        let (app, json) = do_spawn_interrogation(app, &repo_id, &task_id, None).await;
        let agent_id = json["agent"]["id"].as_str().unwrap().to_string();

        // Verify that ABAC policies were created by listing them.
        let policies_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(policies_resp.status(), StatusCode::OK);
        let policies_json = body_json(policies_resp).await;
        let policies = policies_json.as_array().unwrap();

        // Should have 3 interrogation policies for this agent.
        let interrogation_policies: Vec<_> = policies
            .iter()
            .filter(|p| {
                p["name"]
                    .as_str()
                    .map(|n| n.contains(&agent_id))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(
            interrogation_policies.len(),
            3,
            "should create 3 interrogation ABAC policies, found: {policies_json}"
        );
    }

    #[tokio::test]
    async fn interrogation_spawn_without_conversation_sha() {
        // Spawning without conversation_sha should succeed (best-effort context retrieval).
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "No context agent").await;
        let (_, json) = do_spawn_interrogation(app, &repo_id, &task_id, None).await;

        assert_eq!(json["agent"]["status"], "active");
    }

    // ── Compute target resolution tests ────────────────────────────────────────

    /// Helper: create a workspace via the API and return its ID.
    async fn create_workspace(app: Router, name: &str) -> (Router, String) {
        let body = serde_json::json!({"name": name});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "workspace create should succeed"
        );
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// Helper: create a compute target via the API and return its ID.
    async fn create_compute_target(app: Router, name: &str, is_default: bool) -> (Router, String) {
        let body = serde_json::json!({
            "name": name,
            "target_type": "Container",
            "config": {"image": "gyre-agent:latest", "network": "bridge"},
            "is_default": is_default,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "compute target create should succeed"
        );
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// Helper: assign a compute target to a workspace via PUT /api/v1/workspaces/:id.
    async fn assign_compute_target_to_workspace(
        app: Router,
        workspace_id: &str,
        compute_target_id: &str,
    ) -> Router {
        let body = serde_json::json!({"compute_target_id": compute_target_id});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/workspaces/{workspace_id}"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "workspace update should succeed"
        );
        app
    }

    /// Helper: create a repo in a specific workspace.
    async fn create_repo_in_workspace(app: Router, workspace_id: &str) -> (Router, String) {
        let body = serde_json::json!({"workspace_id": workspace_id, "name": "wt-repo"});
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

    #[tokio::test]
    async fn spawn_uses_workspace_assigned_compute_target() {
        // When a workspace has a compute_target_id set and the spawn request
        // does not specify one, the workspace target is resolved automatically.
        let app = app();
        let (app, ws_id) = create_workspace(app, "workspace-ct-test").await;
        let (app, ct_id) = create_compute_target(app, "ws-container", false).await;
        let app = assign_compute_target_to_workspace(app, &ws_id, &ct_id).await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "workspace ct task").await;

        // Spawn without explicit compute_target_id.
        let body = serde_json::json!({
            "name": "ws-ct-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/ws-ct-test",
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
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;

        // Response must include the workspace's assigned compute target.
        assert_eq!(
            json["compute_target_id"].as_str().unwrap_or(""),
            &ct_id,
            "spawn should resolve workspace-assigned compute target: {json}"
        );
    }

    #[tokio::test]
    async fn spawn_falls_back_to_tenant_default_compute_target() {
        // When no explicit compute_target_id is requested and the workspace has
        // no assignment, the tenant's default compute target is used.
        let app = app();
        let (app, ws_id) = create_workspace(app, "workspace-default-ct").await;
        // Create a default compute target for the tenant (is_default = true).
        let (app, ct_id) = create_compute_target(app, "tenant-default", true).await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "tenant default ct task").await;

        // Spawn without explicit compute_target_id, workspace has no assignment.
        let body = serde_json::json!({
            "name": "tenant-default-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/tenant-default-ct",
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
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;

        // Response must include the tenant's default compute target.
        assert_eq!(
            json["compute_target_id"].as_str().unwrap_or(""),
            &ct_id,
            "spawn should fall back to tenant-default compute target: {json}"
        );
    }

    // ── Signal chain: task_type filtering tests (agent-runtime §1 Phase 4) ───

    /// Helper: create a task with a specific task_type via the API.
    async fn create_task_with_type(
        app: Router,
        title: &str,
        task_type: Option<&str>,
    ) -> (Router, String) {
        let mut body = serde_json::json!({"title": title});
        if let Some(tt) = task_type {
            body["task_type"] = serde_json::Value::String(tt.to_string());
        }
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

    /// Helper: attempt spawn and return the response (without asserting status).
    async fn try_spawn(
        app: Router,
        name: &str,
        repo_id: &str,
        task_id: &str,
        branch: &str,
    ) -> (Router, axum::response::Response) {
        let body = serde_json::json!({
            "name": name,
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
        (app, resp)
    }

    #[tokio::test]
    async fn spawn_rejects_delegation_tasks() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) =
            create_task_with_type(app, "Delegation task", Some("delegation")).await;
        let (_, resp) = try_spawn(app, "worker-deleg", &repo_id, &task_id, "feat/deleg").await;

        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "delegation tasks must not trigger worker agent spawning"
        );
        let json = body_json(resp).await;
        let msg = json["error"].as_str().unwrap_or("");
        assert!(
            msg.contains("delegation"),
            "error should mention delegation: {msg}"
        );
    }

    #[tokio::test]
    async fn spawn_rejects_coordination_tasks() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) =
            create_task_with_type(app, "Coordination task", Some("coordination")).await;
        let (_, resp) = try_spawn(app, "worker-coord", &repo_id, &task_id, "feat/coord").await;

        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "coordination tasks must not trigger worker agent spawning"
        );
        let json = body_json(resp).await;
        let msg = json["error"].as_str().unwrap_or("");
        assert!(
            msg.contains("coordination"),
            "error should mention coordination: {msg}"
        );
    }

    #[tokio::test]
    async fn spawn_rejects_tasks_without_task_type() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        // Create task without task_type (simulates push-hook pre-approval task).
        let (app, task_id) = create_task_with_type(app, "Pre-approval push-hook task", None).await;
        let (_, resp) = try_spawn(app, "worker-none", &repo_id, &task_id, "feat/no-type").await;

        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "tasks without task_type must not trigger agent spawning"
        );
        let json = body_json(resp).await;
        let msg = json["error"].as_str().unwrap_or("");
        assert!(
            msg.contains("task_type"),
            "error should mention task_type: {msg}"
        );
    }

    #[tokio::test]
    async fn spawn_allows_implementation_tasks() {
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) =
            create_task_with_type(app, "Implementation task", Some("implementation")).await;
        let (_, resp) = try_spawn(app, "worker-impl", &repo_id, &task_id, "feat/impl").await;

        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "implementation tasks should spawn agents"
        );
    }

    #[tokio::test]
    async fn interrogation_agents_bypass_task_type_check() {
        // Interrogation agents are system-initiated and should work with any task_type,
        // including delegation tasks that would normally be rejected.
        let app = app();
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) =
            create_task_with_type(app, "Delegation for interrogation", Some("delegation")).await;

        let body = serde_json::json!({
            "name": "interrogation-bypass",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "interrogation/bypass",
            "agent_type": "interrogation",
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
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "interrogation agents should bypass task_type filtering"
        );
    }
}
