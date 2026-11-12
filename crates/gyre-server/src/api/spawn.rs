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
    /// Persona slug to bind to this agent (platform-model.md §4.3): resolved
    /// nearest-scope-wins (repo → workspace → tenant), must be Approved, and
    /// its system prompt is injected as `GYRE_PROMPT_PERSONA`. Fail-closed: a
    /// slug that does not resolve to an Approved persona rejects the spawn.
    #[serde(default)]
    pub persona_slug: Option<String>,
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
    /// Whether the compute-target process launch succeeded
    /// (agent-runtime.md §1 Phase 7 spawn-failure handling). False means
    /// the agent record exists but no process is running.
    pub process_launched: bool,
}

/// Agent summary for orchestrator spawns (task-093): `AgentResponse` plus the
/// orchestrator context that the plain conversion drops.
#[derive(Serialize)]
pub struct OrchestratorAgentResponse {
    #[serde(flatten)]
    pub agent: AgentResponse,
    /// Repo the orchestrator is bound to (repo tier only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    /// Whether stale-agent auto-restart will respawn this orchestrator.
    pub restart_on_failure: bool,
}

/// Build the orchestrator response view of a domain agent (task-093).
///
/// The flattened `AgentResponse.repo_id` is left `None`: this struct already
/// serializes `repo_id` at the outer level, and emitting the key twice makes
/// the wire JSON unparseable for strict duplicate-field deserializers
/// (serde default, and every generated client). `persona_id` needs no
/// neutralization because it is declared ONLY on the flattened
/// `AgentResponse` (populated by `AgentResponse::from`) — declaring it here
/// as well is what emitted it twice (task-099 round-3 finding 1).
/// `orchestrator_type` is likewise carried only by the flattened
/// `AgentResponse` (exposed there for the role-scoped chat recipient
/// lookup, HSI §4).
pub(crate) fn orchestrator_response(a: Agent) -> OrchestratorAgentResponse {
    OrchestratorAgentResponse {
        agent: AgentResponse {
            repo_id: None,
            ..AgentResponse::from(a.clone())
        },
        repo_id: a.repo_id.as_ref().map(|id| id.to_string()),
        restart_on_failure: a.restart_on_failure,
    }
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
///
/// Fail closed (task-077 F6): if any restriction/allow policy cannot be
/// created, the error propagates and the caller aborts the spawn — an
/// interrogation agent must never run with only a subset of its restriction
/// policies (the R1 F1 flaw class: a failed `interrogation-restrict-*` Deny
/// left the agent unrestricted with only a log line).
pub async fn create_interrogation_policies(
    state: &AppState,
    agent_id: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let created_ids =
        create_interrogation_policies_in(state.policies.as_ref(), agent_id, now_secs()).await?;

    // Store policy IDs in kv_store for cleanup on complete/kill/stale.
    if let Ok(ids_json) = serde_json::to_string(&created_ids) {
        let _ = state
            .kv_store
            .kv_set("interrogation_policies", agent_id, ids_json)
            .await;
    }

    Ok(created_ids)
}

/// Build the three interrogation policies for an agent (HSI §4):
/// - `interrogation-restrict-{id}`: Deny write/delete/spawn/approve/merge on
///   task/mr/repo/agent/spec/persona/worktree (the agent is read-only).
/// - `interrogation-allow-message-{id}`: Allow write to message resources.
/// - `interrogation-allow-read-{id}`: Allow read to context resources.
///
/// Split from `create_interrogation_policies` so the fail-closed creation
/// loop can be unit-tested against a duplicate-rejecting store without
/// constructing a full `AppState`.
fn interrogation_policies_for(agent_id: &str, now: u64) -> Vec<Policy> {
    let subject_value = format!("agent:{agent_id}");

    let restrict_id = format!("interrogation-restrict-{agent_id}");
    let allow_message_id = format!("interrogation-allow-message-{agent_id}");
    let allow_read_id = format!("interrogation-allow-read-{agent_id}");

    vec![
        // Deny write/delete/spawn/approve/merge on all non-message resources (priority 200).
        Policy {
            id: Id::new(&restrict_id),
            name: restrict_id,
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
            name: allow_message_id,
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
            name: allow_read_id,
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
    ]
}

/// Create the interrogation policies via the given policy repository, failing
/// closed on the first creation error (task-077 F6). Returns the created ids.
async fn create_interrogation_policies_in(
    policies: &dyn gyre_ports::PolicyRepository,
    agent_id: &str,
    now: u64,
) -> Result<Vec<String>, anyhow::Error> {
    let mut created_ids = Vec::new();
    for policy in &interrogation_policies_for(agent_id, now) {
        policies.create(policy).await.map_err(|e| {
            anyhow::anyhow!("failed to create interrogation policy {}: {e}", policy.id)
        })?;
        created_ids.push(policy.id.to_string());
    }
    Ok(created_ids)
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

    let mut any_failed = false;
    for id in &ids {
        if let Err(e) = state.policies.delete(id).await {
            // Fail-visible, fail-retryable (task-077 F6 delete sweep): a
            // failed delete must NOT discard the kv record of the policy id —
            // removing it would orphan the policy with no retry path. Keep
            // the entry so the next cleanup pass (agent.complete, admin kill,
            // stale detection) retries the delete.
            any_failed = true;
            tracing::warn!(
                agent_id = %agent_id,
                policy_id = %id,
                "failed to delete interrogation policy (will retry on next cleanup): {e}"
            );
        }
    }

    // Remove the kv entry only once every policy delete succeeded — otherwise
    // the surviving entry is the retry handle for the orphaned policies.
    if !any_failed {
        let _ = state
            .kv_store
            .kv_remove("interrogation_policies", agent_id)
            .await;
    }

    tracing::info!(
        agent_id = %agent_id,
        count = ids.len(),
        "interrogation policies cleaned up"
    );
}

/// Clean up an agent's jj workspace: forget it from the repo's shared jj
/// checkout and remove the on-disk working copy. Called on every agent
/// teardown path (stale/abort, admin kill, worktree delete).
///
/// `main_checkout_path` is the repo's shared jj checkout
/// (`{repo.path}/jj-main`); `workspace_path`/`workspace_name` identify the
/// agent's working copy. Best-effort: teardown must not fail because jj
/// state is already gone, but failures are logged at warn (a leaked
/// workspace stays registered in `jj workspace list` otherwise).
pub async fn cleanup_jj_workspace(
    state: &AppState,
    agent_id: &str,
    repo_path: &str,
    workspace_path: &str,
    workspace_name: &str,
) {
    let main_checkout = format!("{repo_path}/jj-main");
    if let Err(e) = state
        .jj_ops
        .jj_workspace_forget(&main_checkout, workspace_name)
        .await
    {
        tracing::warn!(
            agent_id = %agent_id,
            workspace = %workspace_name,
            error = %e,
            "jj workspace forget failed on cleanup"
        );
    }
    if std::path::Path::new(workspace_path).exists() {
        if let Err(e) = std::fs::remove_dir_all(workspace_path) {
            tracing::warn!(
                agent_id = %agent_id,
                path = %workspace_path,
                error = %e,
                "failed to remove jj workspace directory"
            );
        }
    }
    // Remove the now-empty parent dir (workspaces/) when this was the last
    // workspace — keep the tree tidy; ignore failures.
    if let Some(parent) = std::path::Path::new(workspace_path).parent() {
        let _ = std::fs::remove_dir(parent);
    }
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
pub(crate) async fn spawn_agent_core(
    state: &Arc<AppState>,
    req: SpawnAgentRequest,
    auth: &AuthenticatedAgent,
    endpoint: &str,
) -> Result<SpawnAgentResponse, ApiError> {
    // non-atomic-create:ok - agent+worktree creation with compensating
    // rollback: if the worktree record (or the interrogation restrict-Deny)
    // fails to create, the just-created agent record and its token are
    // deleted (see both rollback paths below), so a failed spawn leaves no
    // orphaned entities.
    // Verify repo exists
    let repo = state
        .repos
        .find_by_id(&Id::new(&req.repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", req.repo_id)))?;

    // G6: ABAC enforcement — check repo access policies against the caller's JWT claims.
    if let Err(reason) = crate::abac::check_repo_abac(state, &req.repo_id, auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware. `endpoint` names the calling
        // surface (REST route or MCP tool) since this core is shared.
        crate::abac_middleware::record_abac_denied(state, auth, "repo", "spawn", None, endpoint)
            .await;
        return Err(ApiError::Forbidden(reason));
    }

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
    // Tenant guard (task-117 F2): a request-supplied compute_target_id must exist
    // AND belong to the tenant that scopes this spawn (the repo's workspace
    // tenant — where the agent will run). A foreign-tenant id is
    // indistinguishable from a missing one: 404, no existence confirmation,
    // mirroring the compute-targets handlers.
    let spawn_tenant_id: Id = workspace
        .as_ref()
        .map(|ws| ws.tenant_id.clone())
        .unwrap_or_else(|| Id::new(&auth.tenant_id));
    let resolved_ct_entity: Option<ComputeTargetEntity> =
        if let Some(ref ct_id) = req.compute_target_id {
            let entity = state
                .compute_targets
                .get_by_id(&Id::new(ct_id))
                .await
                .ok()
                .flatten();
            match entity {
                Some(e) if e.tenant_id == spawn_tenant_id => Some(e),
                _ => {
                    return Err(ApiError::NotFound(format!(
                        "compute target {ct_id} not found"
                    )));
                }
            }
        } else if let Some(ws_ct_id) = workspace
            .as_ref()
            .and_then(|ws| ws.compute_target_id.clone())
        {
            // Workspace-assigned compute target. Assignment is validated at
            // update time, but re-check the tenant here so a stale or
            // tampered assignment cannot bind this spawn to another
            // tenant's infrastructure.
            let entity = state
                .compute_targets
                .get_by_id(&ws_ct_id)
                .await
                .ok()
                .flatten();
            match entity {
                Some(e) if e.tenant_id == spawn_tenant_id => Some(e),
                Some(_) => {
                    return Err(ApiError::NotFound(format!(
                        "compute target {} not found",
                        ws_ct_id.as_str()
                    )));
                }
                None => None,
            }
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

    // Agent names become container names / remote processes on SSH
    // compute targets — validate BEFORE any state is written (agent
    // record, token, worktree, task assignment). The name check used to
    // live inside the SSH dispatch arm below, i.e. after every one of
    // those writes, so a rejected name left an Active agent, an
    // InProgress task assigned to it, and a worktree record behind —
    // violating this handler's compensating-rollback invariant (the
    // worktree-creation-failure and interrogation-policy-failure paths
    // both delete the just-created agent). (M19.5-A.)
    if resolved_ct_entity
        .as_ref()
        .map(|e| e.target_type == ComputeTargetType::Ssh)
        .unwrap_or(false)
    {
        let safe_name = req
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
        if !safe_name || req.name.is_empty() || req.name.len() > 63 {
            return Err(ApiError::InvalidInput(
                "agent name must be 1-63 chars of [a-zA-Z0-9._-] for container use".to_string(),
            ));
        }
    }

    // platform-model.md §4.3: resolve the spawn's requested persona through
    // the repo → workspace → tenant chain and bind it to the agent row.
    // Fail-closed on a slug that does not resolve to an Approved persona
    // (same posture as orchestrator spawns): an agent styled by a missing or
    // unapproved persona is exactly what the spec forbids.
    let persona: Option<gyre_domain::Persona> = match &req.persona_slug {
        Some(slug) if !slug.is_empty() => {
            let ws = workspace.as_ref().ok_or_else(|| {
                ApiError::InvalidInput(format!(
                    "cannot resolve persona '{slug}': workspace {} unresolvable",
                    repo.workspace_id
                ))
            })?;
            let prompt_scope = crate::api::prompts::PromptScope {
                tenant_id: Some(ws.tenant_id.clone()),
                workspace_id: repo.workspace_id.clone(),
                repo_id: Some(repo.id.clone()),
                task_id: Some(Id::new(&req.task_id)),
                agent_id: None,
            };
            Some(
                crate::api::prompts::resolve_persona_in_chain(state, slug, &prompt_scope)
                    .await
                    .map_err(ApiError::InvalidInput)?,
            )
        }
        _ => None,
    };

    // Create agent with Active status
    let mut agent = Agent::new(new_id(), req.name, now);
    agent.parent_id = req.parent_id.map(Id::new);
    agent.spawned_by = Some(auth.agent_id.clone());
    // §4.3 persona binding: persisted on the agent row so system://persona
    // can resolve it without a slug argument later.
    if let Some(p) = &persona {
        agent.persona_id = Some(p.id.clone());
    }
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

    // agent-runtime.md §3 (Nix-Based Image Build): "The image hash is
    // recorded in workload attestation (`wl_image_hash` JWT claim)." The
    // token must exist before the process starts (it is injected into the
    // spawn environment), so the digest is resolved from the image runtime
    // BEFORE minting — `docker image inspect` needs no running container
    // and returns exactly the local image `docker run` will use. Best-
    // effort: an unresolvable image (runtime absent, image not pulled yet,
    // SSH host down) mints the claim absent. Kubernetes cannot know the
    // digest before scheduling (the pod `imageID` only exists once the Pod
    // runs), so that backend records the digest in the post-spawn workload
    // attestation record instead.
    let wl_image_hash: Option<String> = match resolved_ct_entity.as_ref() {
        Some(e) if e.target_type == ComputeTargetType::Container => {
            let image = e.config["image"].as_str().unwrap_or("gyre-agent:latest");
            gyre_adapters::compute::ContainerTarget::new(image.to_string())
                .image_digest()
                .await
                .ok()
        }
        Some(e)
            if e.target_type == ComputeTargetType::Ssh
                && e.config["container_mode"].as_bool().unwrap_or(true) =>
        {
            gyre_adapters::compute::ssh::ssh_docker_target_from_config(&e.config)
                .remote_image_digest()
                .await
                .ok()
        }
        _ => None,
    };

    // Pre-mint a JWT so it can be injected into the container environment at
    // spawn time.  After spawn we create the workload attestation record
    // (stored in state.workload_attestations) which is queryable via
    // GET /api/v1/agents/{id}/workload.
    let token = state
        .agent_signing_key
        .mint_with_workload(
            &agent.id.to_string(),
            &req.task_id,
            &auth.agent_id,
            &state.base_url,
            jwt_ttl,
            None,
            None,
            None,
            None,
            None,
            wl_image_hash,
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
    // Fail closed (task-077 F6): if the restriction policies cannot be created,
    // abort the spawn — never leave an interrogation agent running unrestricted.
    if is_interrogation {
        if let Err(e) = create_interrogation_policies(state, agent.id.as_str()).await {
            // Roll back the already-created agent record and token so the
            // aborted spawn leaves no orphaned entities behind.
            if let Err(del_err) = state.agents.delete(&agent.id).await {
                tracing::error!(
                    agent_id = %agent.id,
                    error = %del_err,
                    "failed to delete agent record after interrogation policy creation failure"
                );
            }
            let _ = state
                .kv_store
                .kv_remove("agent_tokens", &agent.id.to_string())
                .await;
            return Err(ApiError::Internal(e));
        }

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

    // Compute provisioning paths (source-control.md §4 layout):
    //   {repo_root}/jj-main      — the repo's shared jj main checkout
    //                              (standalone, backed by the bare git repo)
    //   {repo_root}/workspaces/{agent_id} — this agent's jj workspace
    //                              (working copy on the spawn branch)
    //   {repo_root}/worktrees/{branch_slug} — plain-git worktree fallback
    // jj 0.39.0 refuses colocated repos inside git worktrees, so agent jj
    // working copies are separate workspaces of the shared checkout.
    //
    // F6: `repo.path` is absolute at rest (repos_root is canonicalized at
    // server start), but rows persisted BEFORE that fix may still be
    // relative. jj resolves every PATH ARGUMENT (`--git-repo`, the
    // workspace path) against the command's cwd — the checkout dir, not
    // the server's — so a legacy relative row silently breaks provisioning
    // exactly like a relative root does. Absolutize against the server cwd
    // once, here, before any jj/git child call consumes the path. When the
    // dir exists, canonicalize (resolves symlinks and `..` segments);
    // otherwise anchor the raw path at the cwd.
    let repo_root = {
        let p = std::path::Path::new(&repo.path);
        if p.is_absolute() {
            repo.path.clone()
        } else if let Ok(canon) = std::fs::canonicalize(p) {
            canon.to_string_lossy().into_owned()
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
                Err(_) => repo.path.clone(),
            }
        }
    };
    let branch_slug = req.branch.replace('/', "-");
    let worktree_path = format!("{}/worktrees/{}", repo_root, branch_slug);
    // Response value for `worktree_path`: the directory the agent
    // actually works in, i.e. the same path persisted to the worktree
    // record. When jj provisioning succeeds the plain-git worktree is
    // NOT created — the agent's directory is the jj workspace — so
    // reporting `worktree_path` there names a nonexistent directory
    // (review de1eec5d F1). Set below alongside `jj_change_id`.
    let response_worktree_path;
    let jj_main_checkout_path = format!("{}/jj-main", repo_root);
    let jj_workspace_path = format!("{}/workspaces/{}", repo_root, agent.id);
    let jj_workspace_name = format!("agent-{}", agent.id);

    // HSI §4: Interrogation agents are read-only — they have no worktree.
    // Skip worktree creation, jj init, and git ref writes.
    let jj_change_id = if !is_interrogation {
        // F9: the spawn branch must exist as a git ref BEFORE jj
        // provisioning — `jj workspace add -r <branch>` resolves the
        // branch against the bare repo's imported git refs and fails for
        // an unknown branch ("Revision '<branch>' doesn't exist"), which
        // silently degrades the agent to a plain-git worktree that the
        // automatic rebase (source-control.md §4) cannot operate on.
        // New-branch spawns are the designed-for common case (the agent
        // entrypoint's `git checkout -b` fallback, the git-worktree
        // fallback's `git worktree add -b`, MCP gyre_spawn_worker's
        // orchestrator-chosen branch). Create the branch from the repo
        // HEAD when missing — the same base the git-worktree fallback
        // uses — so jj and the plain-git surface (refs, can_merge,
        // merges) both see it. Failure is non-fatal: jj provisioning
        // then fails naturally and the plain-git fallback takes over.
        match state.git_ops.branch_exists(&repo_root, &req.branch).await {
            Ok(true) => {}
            Ok(false) => {
                if let Err(e) = state
                    .git_ops
                    .create_branch(&repo_root, &req.branch, "HEAD")
                    .await
                {
                    tracing::warn!(
                        agent_id = %agent.id,
                        branch = %req.branch,
                        error = %e,
                        "cannot create spawn branch in bare repo — jj provisioning will fall back to plain git"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    branch = %req.branch,
                    error = %e,
                    "cannot check spawn branch in bare repo — jj provisioning may fall back to plain git"
                );
            }
        }

        // Provision the agent's jj working copy FIRST: a per-agent
        // workspace of the repo's shared jj checkout, created on the
        // spawn branch. This is the layout the automatic rebase
        // (source-control.md §4) operates on.
        //
        // Failure degrades to the plain-git worktree path below — the
        // agent still gets a working directory (best-effort, but the
        // failure is logged at warn, never silently swallowed: a spawn
        let mut change_id: Option<String> = None;
        let jj_provisioned = match state
            .jj_ops
            .jj_main_checkout_init(&jj_main_checkout_path, &repo_root)
            .await
        {
            Ok(()) => {
                let description = format!("Agent {}: task {}", agent.name, req.task_id);
                match state
                    .jj_ops
                    .jj_workspace_add(
                        &jj_main_checkout_path,
                        &jj_workspace_path,
                        &jj_workspace_name,
                        &req.branch,
                        &description,
                    )
                    .await
                {
                    Ok(()) => {
                        // The workspace's working copy IS the agent's new
                        // change — describe it so the change is named.
                        match state.jj_ops.jj_new(&jj_workspace_path, &description).await {
                            Ok(cid) => {
                                tracing::info!(
                                    agent_id = %agent.id,
                                    workspace = %jj_workspace_path,
                                    change_id = %cid,
                                    "jj workspace provisioned for agent"
                                );
                                change_id = Some(cid);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    agent_id = %agent.id,
                                    error = %e,
                                    "jj new in workspace failed — workspace stays on branch tip"
                                );
                            }
                        }
                        true
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent_id = %agent.id,
                            error = %e,
                            "jj workspace add failed — falling back to plain git worktree"
                        );
                        false
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    checkout = %jj_main_checkout_path,
                    error = %e,
                    "jj main checkout init failed — falling back to plain git worktree"
                );
                false
            }
        };

        // Plain-git worktree: created for every agent (the plain-git
        // surface — CLI clones, git push, can_merge — keys off git refs),
        // and the FALLBACK working directory when jj provisioning failed.
        // Skipped when the jj workspace provisioned (its working copy
        // serves as the agent's directory).
        if !jj_provisioned {
            if let Err(e) = state
                .git_ops
                .create_worktree(&repo_root, &worktree_path, &req.branch)
                .await
            {
                let msg = e.to_string();
                let msg_lc = msg.to_lowercase();
                if msg_lc.contains("not a valid object") || msg_lc.contains("bad default revision")
                {
                    return Err(ApiError::InvalidInput(format!(
                        "cannot create worktree: repo has no commits yet — push an initial commit before spawning (branch: {})",
                        req.branch
                    )));
                }
                tracing::warn!("create_worktree failed (non-fatal): {e}");
            }
        }

        // Write custom ref namespaces (best-effort)
        if let Some(sha) = git_refs::resolve_ref(&repo_root, "HEAD").await {
            let agent_ref = format!("refs/agents/{}/head", agent.id);
            let task_ref = format!("refs/tasks/{}", task.id);
            git_refs::write_ref(&repo_root, &agent_ref, &sha).await;
            git_refs::write_ref(&repo_root, &task_ref, &sha).await;
        }

        // Record worktree in DB linked to agent and task. The recorded
        // path is the jj workspace when provisioned (that's where the
        // agent's in-progress change lives — and what the automatic
        // rebase operates on), else the plain-git worktree.
        let effective_path = if jj_provisioned {
            jj_workspace_path.clone()
        } else {
            worktree_path.clone()
        };
        // The response must report the same path the worktree record
        // persists — the agent's actual working directory (jj workspace
        // when provisioned, plain-git worktree otherwise). The bare
        // `worktree_path` names a directory that is never created when
        // jj provisioning succeeded (review de1eec5d F1).
        response_worktree_path = effective_path.clone();
        let wt = AgentWorktree::new(
            new_id(),
            agent.id.clone(),
            Id::new(&req.repo_id),
            Some(Id::new(&req.task_id)),
            req.branch.clone(),
            effective_path,
            now,
        );
        if let Err(wt_err) = state.worktrees.create(&wt).await {
            // Compensating rollback (same pattern as the interrogation
            // fail-closed path above): the agent record was already created;
            // leaving it behind would occupy the workspace's agent slot and
            // show a spawnable agent with no worktree. Delete it (and its
            // token) so a failed spawn leaves no orphaned entities.
            if let Err(del_err) = state.agents.delete(&agent.id).await {
                tracing::error!(
                    agent_id = %agent.id,
                    error = %del_err,
                    "failed to delete agent record after worktree creation failure"
                );
            }
            let _ = state
                .kv_store
                .kv_remove("agent_tokens", &agent.id.to_string())
                .await;
            return Err(wt_err.into());
        }

        change_id
    } else {
        // Interrogation agents are read-only with no worktree at all;
        // keep the historical placeholder path in the response.
        response_worktree_path = worktree_path.clone();
        None
    };

    // Agent-runtime §2: assemble the meta-spec prompt set for this agent —
    // required tenant → required workspace → spec-level bindings at pinned
    // versions. The set is injected into the agent environment and recorded
    // (kv: agent_meta_spec_sets) so the merge attestation can cite exactly
    // which meta-specs the agent ran under.
    let prompt_set = crate::prompt_assembly::assemble_prompt_set(state, &task).await;
    if !prompt_set.sections.is_empty() {
        crate::prompt_assembly::store_prompt_set_record(
            state,
            &crate::prompt_assembly::AgentPromptSetRecord {
                agent_id: agent.id.clone(),
                task_id: task.id.clone(),
                spec_path: task.spec_path.clone(),
                set_sha: prompt_set.set_sha.clone(),
                assembled_at: now,
                meta_specs_used: prompt_set.sections.iter().map(|s| s.used.clone()).collect(),
            },
        )
        .await;
    }

    // Assign task to agent and advance to InProgress
    //
    // §2.4 replay prevention: spawning onto a task that was assigned to a
    // different agent is a reassignment — bump the deployment generation so
    // SignedInputs pinned to the previous generation cannot be replayed.
    if task.assigned_to.as_ref() != Some(&agent.id) {
        task.generation = task.generation.saturating_add(1);
    }
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

    // Launch a real process and monitor its lifecycle. Shared with the
    // Ralph-loop re-spawn path (agent-runtime.md §1 Phase 7) so first
    // spawn and re-spawn run identical provisioning.
    let launch = launch_agent_process(
        state,
        &AgentLaunchCtx {
            agent: agent.clone(),
            workspace: workspace.clone(),
            repo: repo.clone(),
            task: task.clone(),
            branch: req.branch.clone(),
            token: token.clone(),
            clone_url: clone_url.clone(),
            prompt_set: prompt_set.clone(),
            resolved_ct_entity: resolved_ct_entity.clone(),
            jj_workspace_path: jj_workspace_path.clone(),
            plain_worktree_path: worktree_path.clone(),
            persona: persona.clone(),
            is_interrogation,
        },
    )
    .await;
    let spawned_pid = launch.pid;
    let spawned_container_id = launch.container_id;
    let spawned_container_image = launch.container_image;
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
            &launch.compute_target_label,
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
            state,
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
    super::budget::increment_active_agents(state, &repo.workspace_id.to_string()).await;

    // Agent-runtime §2: meta-spec set SHA from the assembled prompt set
    // (required tenant → required workspace → spec bindings at pinned
    // versions). Empty set → None (no meta-specs apply to this spawn).
    let meta_spec_set_sha: Option<String> = if prompt_set.sections.is_empty() {
        None
    } else {
        Some(prompt_set.set_sha.clone())
    };

    // Agent lifecycle audit event (observability.md §Audit Event Types).
    crate::agent_audit::emit_spawned(
        state,
        &agent,
        &req.task_id,
        &launch.compute_target_label,
        &req.branch,
        &auth.agent_id,
    )
    .await;

    Ok(SpawnAgentResponse {
        agent: {
            let mut r = AgentResponse::from(agent);
            r.repo_id = Some(req.repo_id.clone());
            r.branch = Some(req.branch.clone());
            r.task_id = Some(req.task_id.clone());
            r
        },
        token,
        worktree_path: response_worktree_path,
        clone_url,
        branch: req.branch,
        compute_target_id: resolved_ct_entity.as_ref().map(|e| e.id.to_string()),
        jj_change_id,
        container_id: spawned_container_id,
        meta_spec_set_sha,
        process_launched: launch.launched,
    })
}

// ── Compute-target launch (shared by spawn + Ralph-loop re-spawn) ─────────────

/// Everything the compute-target launch of one agent session needs.
///
/// Shared by [`spawn_agent_core`] (Phase 4 first spawn) and the Ralph-loop
/// re-spawn path (agent-runtime.md §1 Phase 7: same persona, same meta-spec
/// set, same task assignment, fresh context) so the two can never drift.
pub(crate) struct AgentLaunchCtx {
    /// The agent record (already persisted, already `Active`).
    pub agent: Agent,
    /// The agent's workspace record (secret resolution, tenant scope).
    pub workspace: Option<gyre_domain::Workspace>,
    /// The repo the agent works in.
    pub repo: gyre_domain::Repository,
    /// The task being implemented (id feeds GYRE_TASK_ID + secret scoping).
    pub task: gyre_domain::Task,
    /// Branch the agent works on.
    pub branch: String,
    /// The agent's (freshly minted) auth token, injected as GYRE_AUTH_TOKEN.
    pub token: String,
    /// Git Smart HTTP clone URL for the repo.
    pub clone_url: String,
    /// Assembled meta-spec prompt set (agent-runtime.md §2) — rendered into
    /// GYRE_META_SPEC_PROMPT / GYRE_META_SPEC_SET_SHA.
    pub prompt_set: crate::prompt_assembly::PromptSet,
    /// Resolved compute target entity (request → workspace → tenant default).
    pub resolved_ct_entity: Option<ComputeTargetEntity>,
    /// The agent's jj workspace path (preferred working dir).
    pub jj_workspace_path: String,
    /// Plain-git worktree path (fallback working dir).
    pub plain_worktree_path: String,
    /// The persona bound to the agent row at spawn time
    /// (platform-model.md §4.3): its `system_prompt` is injected as
    /// GYRE_PROMPT_PERSONA. `None` when the agent has no bound persona —
    /// an unbound worker has no persona prompt, which is honest, not a
    /// gap. The re-spawn path resolves the same binding so first spawn
    /// and re-spawn never drift.
    pub persona: Option<gyre_domain::Persona>,
    /// Interrogation agents have no worktree; their context prompt
    /// records that honestly instead of citing one.
    pub is_interrogation: bool,
}

/// Outcome of one compute-target launch attempt.
pub(crate) struct AgentLaunchResult {
    /// Whether `ComputeTarget::spawn_process()` succeeded on any backend.
    /// False means the process never started (Docker down, SSH unreachable,
    /// K8s API down) — the Ralph loop's spawn-failure retry keys off this.
    pub launched: bool,
    pub pid: Option<u32>,
    pub container_id: Option<String>,
    pub container_image: Option<String>,
    /// Label of the target the launch resolved to (attestation/audit).
    pub compute_target_label: String,
}

/// Launch the agent process on the resolved compute target and register it
/// for lifecycle management (process registry, backend routing, exit
/// monitor). Mirrors agent-runtime.md §1 Phase 4 steps 9–10 and §3.
///
/// Launch failure is reported via [`AgentLaunchResult::launched`] rather
/// than an `Err`: the caller decides the failure policy (the REST spawn
/// surface keeps the agent record for manual retry; the task scheduler's
/// worker path retries with backoff per Phase 7).
pub(crate) async fn launch_agent_process(
    state: &Arc<AppState>,
    ctx: &AgentLaunchCtx,
) -> AgentLaunchResult {
    let compute_target_label = ctx
        .resolved_ct_entity
        .as_ref()
        .map(|e| e.id.to_string())
        .unwrap_or_else(|| "local".to_string());

    let resolved_target_config: Option<super::compute::ComputeTargetConfig> = ctx
        .resolved_ct_entity
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

    let agent = &ctx.agent;
    let repo = &ctx.repo;
    let task = &ctx.task;
    let workspace = ctx.workspace.as_ref();
    let token = &ctx.token;
    let jj_workspace_path = &ctx.jj_workspace_path;
    let worktree_path = &ctx.plain_worktree_path;
    let prompt_set = &ctx.prompt_set;
    let req_branch = &ctx.branch;
    let clone_url = &ctx.clone_url;
    let mut launched = false;
    let spawned_pid: Option<u32>;
    let spawned_container_id: Option<String>; // M19.1/M19.3/M19.4
    let spawned_container_image: Option<String>; // M19.3/M19.4

    // platform-model.md §4.3 MCP Prompts: build the four system prompts for
    // this agent. They are injected into the container environment below —
    // part of the agent's initial context, not fetched. The persona prompt is
    // only present when a persona is bound (an unbound worker has none);
    // protocol/context/constraints always are. Interrogation agents have no
    // worktree, so their context prompt records that honestly.
    let prompt_scope = crate::api::prompts::PromptScope {
        tenant_id: workspace.as_ref().map(|ws| ws.tenant_id.clone()),
        workspace_id: repo.workspace_id.clone(),
        repo_id: Some(repo.id.clone()),
        task_id: Some(task.id.clone()),
        agent_id: Some(agent.id.clone()),
    };
    let prompt_protocol = crate::api::prompts::protocol_prompt().to_string();
    let prompt_context = crate::api::prompts::context_prompt_for_spawn(
        task,
        repo,
        if ctx.is_interrogation {
            None
        } else {
            Some((req_branch.as_str(), worktree_path.as_str()))
        },
    );
    let prompt_constraints = crate::api::prompts::constraints_prompt(state, &prompt_scope).await;
    let prompt_persona = ctx.persona.as_ref().map(|p| p.system_prompt.clone());

    {
        // Docker requires an absolute working directory path. Canonicalize
        // the worktree path so legacy relative paths (older repo.path rows
        // persisted before repos_root was made absolute, or a relative
        // GYRE_REPOS_PATH in the deployment config) still resolve.
        // task-106 layout: prefer the agent's jj workspace dir when it
        // exists, else the plain-git worktree.
        let effective_work_dir = if std::path::Path::new(&jj_workspace_path).exists() {
            std::fs::canonicalize(&jj_workspace_path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| jj_workspace_path.clone())
        } else if std::path::Path::new(&worktree_path).exists() {
            std::fs::canonicalize(&worktree_path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| worktree_path.clone())
        } else {
            // No working directory on disk — fall back to /workspace (absolute).
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
        // Agent-runtime §2: meta-spec system prompt. The assembled set
        // (required tenant → required workspace → spec bindings) is the
        // agent's behavioral instruction context.
        let meta_prompt = crate::prompt_assembly::render_system_prompt(&prompt_set.sections);
        if !meta_prompt.is_empty() {
            container_env.insert("GYRE_META_SPEC_PROMPT".to_string(), meta_prompt);
            container_env.insert(
                "GYRE_META_SPEC_SET_SHA".to_string(),
                prompt_set.set_sha.clone(),
            );
        }
        container_env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
        container_env.insert("GYRE_AUTH_TOKEN".to_string(), token.clone());
        container_env.insert("GYRE_CLONE_URL".to_string(), clone_url.clone());
        container_env.insert("GYRE_BRANCH".to_string(), req_branch.clone());
        container_env.insert("GYRE_AGENT_ID".to_string(), agent.id.to_string());
        container_env.insert("GYRE_TASK_ID".to_string(), task.id.to_string());
        container_env.insert("GYRE_REPO_ID".to_string(), repo.id.to_string());
        // Ralph-loop position (agent-runtime.md §1 Phase 7 / §5 prompt
        // structure): 0 on first spawn, 1+ on every re-spawn so a fresh
        // session knows which iteration it is running.
        container_env.insert("GYRE_ITERATION".to_string(), agent.iteration.to_string());

        // platform-model.md §4.3: the system prompts are injected at spawn
        // time — the agent does not need to fetch them. Content is identical
        // to what the MCP `prompts/get` handlers serve.
        if let Some(persona_prompt) = &prompt_persona {
            container_env.insert("GYRE_PROMPT_PERSONA".to_string(), persona_prompt.clone());
        }
        container_env.insert("GYRE_PROMPT_PROTOCOL".to_string(), prompt_protocol.clone());
        container_env.insert("GYRE_PROMPT_CONTEXT".to_string(), prompt_context.clone());
        container_env.insert(
            "GYRE_PROMPT_CONSTRAINTS".to_string(),
            prompt_constraints.clone(),
        );

        // Platform Model §7 Secrets Delivery: resolve scoped secrets from the
        // repository (tenant → workspace → repo → task, nearest scope wins)
        // and inject as GYRE_CRED_* env vars. The cred-proxy sidecar reads
        // GYRE_CRED_* vars, stores in memory, and scrubs them before the agent
        // process starts — raw values are never in the agent env.
        // Failure is logged and skipped: a missing/undecryptable secret must
        // not block spawning the agent itself.
        // Resolve the tenant from the workspace record; never fabricate a
        // "default" tenant. On an unresolvable workspace, skip tenant-scoped
        // (and thus all) secret resolution and warn — the spawn itself must
        // continue without GYRE_CRED_* env vars.
        match workspace.as_ref() {
            Some(ws) => {
                let tenant_id = ws.tenant_id.to_string();
                match state
                    .secrets
                    .resolve_for_agent(
                        &tenant_id,
                        &repo.workspace_id.to_string(),
                        repo.id.as_str(),
                        Some(task.id.as_str()),
                    )
                    .await
                {
                    Ok(resolved) => {
                        for (name, value) in resolved {
                            // Platform Model §7 audit event: secret.accessed
                            // at spawn time — secret name (never value), the
                            // agent's resolution scope, and agent_id.
                            tracing::info!(
                                event = "secret.accessed",
                                agent_id = %agent.id,
                                task_id = %task.id,
                                repo_id = %repo.id,
                                workspace_id = %repo.workspace_id,
                                tenant_id = %tenant_id,
                                secret_name = %name,
                                scope = "tenant>workspace>repo>task",
                                "secret.accessed: secret resolved for agent spawn (GYRE_CRED_* injection)"
                            );
                            // Env vars must be UTF-8. Skip (and name, never
                            // log) non-UTF-8 values rather than corrupting
                            // them with replacement chars — same availability
                            // posture as the resolve-failure branch.
                            match String::from_utf8(value) {
                                Ok(s) => {
                                    container_env.insert(format!("GYRE_CRED_{name}"), s);
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        agent_id = %agent.id,
                                        secret_name = %name,
                                        "skipping non-UTF-8 secret; GYRE_CRED_* env vars require UTF-8 values"
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent_id = %agent.id,
                            "secret resolution failed; continuing without GYRE_CRED_* env vars: {e:#}"
                        );
                    }
                }
            }
            None => {
                tracing::warn!(
                    agent_id = %agent.id,
                    workspace_id = %repo.workspace_id,
                    repo_id = %repo.id,
                    "workspace unresolvable; skipping scoped secret resolution (no GYRE_CRED_* env vars)"
                );
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
        // Secrets (e.g. GCP SA JSON) are delivered via the secrets repository above.
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
                        launched = true;
                        spawned_container_id = Some(handle.id.clone());
                        // Workload-attestation image hash: the resolved
                        // image digest (sha256:...), not the image name —
                        // `derive_attestation_level` treats a non-empty
                        // image_hash as a Level-3 "Gyre-managed runtime"
                        // signal, so a bare tag must not inflate it.
                        // Best-effort: no runtime / unresolvable image →
                        // None (level stays 2).
                        spawned_container_image = ct.image_digest().await.ok();

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

                        register_spawned_agent(
                            state,
                            &agent,
                            handle.clone(),
                            SpawnBackend::Container(ct),
                            &resolved_target_config,
                        )
                        .await;
                    }
                    Err(e) => {
                        spawned_pid = None;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        tracing::warn!(
                            agent_id = %agent.id,
                            "container spawn failed (best-effort): {e}. \
                            If the image is missing, build it first: \
                            `docker build -t gyre-agent:latest docker/gyre-agent/`."
                        );
                    }
                }
            }
            Some(cfg) if cfg.target_type == "ssh" => {
                // agent-runtime.md §3 Supported Backends — SSH: "SSH to
                // remote host, `docker run` there". SshDockerTarget runs
                // docker on the remote host with the same security
                // defaults as the container backend and tracks the remote
                // *container* (not the short-lived docker client process):
                // the handle id is the remote container ID, `kill_process`
                // runs `docker rm --force` remotely, and `is_alive` runs
                // `docker inspect` remotely.
                //
                // `container_mode: false` is the legacy M19.5 escape hatch
                // (bare remote process via SshTarget) kept for configs that
                // predate the spec default.
                //
                // NOTE: the agent-name charset/length validation for this
                // arm lives at the top of spawn_agent_core, beside the
                // duplicate-name check — it must run before any state is
                // written (see the comment there).
                let container_mode = cfg.config["container_mode"].as_bool().unwrap_or(true);

                // Worktree path: local absolute path for bare remote mode;
                // the remote container clones its own checkout into
                // /workspace (entrypoint.sh), so /workspace is the workdir.
                let remote_work_dir = if container_mode {
                    "/workspace".to_string()
                } else {
                    effective_work_dir
                };

                // Remote spawn uses the same env-injected SpawnConfig; the
                // ssh backend forwards env via `--env K=V` docker args.
                let ssh_spawn_config = gyre_ports::SpawnConfig {
                    work_dir: remote_work_dir,
                    ..spawn_config.clone()
                };

                if container_mode {
                    let ssh_docker =
                        gyre_adapters::compute::ssh::ssh_docker_target_from_config(&cfg.config);
                    match gyre_ports::ComputeTarget::spawn_process(&ssh_docker, &ssh_spawn_config)
                        .await
                    {
                        Ok(handle) => {
                            spawned_pid = handle.pid;
                            launched = true;
                            spawned_container_id = Some(handle.id.clone());
                            // Resolved digest from the REMOTE docker daemon
                            // (see container arm: image_hash must be a
                            // digest, not a tag, to count as Level 3).
                            spawned_container_image = ssh_docker.remote_image_digest().await.ok();
                            register_spawned_agent(
                                state,
                                &agent,
                                handle.clone(),
                                SpawnBackend::SshDocker(ssh_docker),
                                &resolved_target_config,
                            )
                            .await;
                        }
                        Err(e) => {
                            spawned_pid = None;
                            spawned_container_id = None;
                            spawned_container_image = None;
                            tracing::warn!(
                                agent_id = %agent.id,
                                "SSH docker spawn failed (best-effort): {e}"
                            );
                        }
                    }
                } else {
                    // Legacy bare remote-process mode (M19.5).
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
                    match gyre_ports::ComputeTarget::spawn_process(&ssh_target, &ssh_spawn_config)
                        .await
                    {
                        Ok(handle) => {
                            spawned_pid = handle.pid;
                            launched = true;
                            spawned_container_id = None;
                            spawned_container_image = None;
                            register_spawned_agent(
                                state,
                                &agent,
                                handle.clone(),
                                SpawnBackend::SshBare(ssh_target),
                                &resolved_target_config,
                            )
                            .await;
                        }
                        Err(e) => {
                            spawned_pid = None;
                            spawned_container_id = None;
                            spawned_container_image = None;
                            tracing::warn!(
                                agent_id = %agent.id,
                                "SSH spawn failed (best-effort): {e}"
                            );
                        }
                    }
                }
            }
            Some(cfg) if cfg.target_type == "kubernetes" => {
                // agent-runtime.md §3: Kubernetes backend — create a Pod
                // with the agent image. kill_process deletes the Pod;
                // is_alive checks the Pod phase.
                let k8s =
                    gyre_adapters::compute::kubernetes::kubernetes_target_from_config(&cfg.config);

                // The Pod mounts no volume for the server-local worktree
                // (build_pod_manifest emits no volumes), and the agent
                // image clones its own checkout into /workspace
                // (entrypoint.sh) — the image WorkingDir is /workspace, so
                // a server-local absolute path as workingDir makes the
                // container fail to start under CRI (runc chdir ENOENT).
                // Mirror the SSH container arm: /workspace is the workdir.
                let k8s_spawn_config = gyre_ports::SpawnConfig {
                    work_dir: "/workspace".to_string(),
                    ..spawn_config.clone()
                };
                match gyre_ports::ComputeTarget::spawn_process(&k8s, &k8s_spawn_config).await {
                    Ok(handle) => {
                        spawned_pid = handle.pid;
                        launched = true;
                        spawned_container_id = Some(handle.id.clone());
                        // Digest the cluster actually pulled (imageID), not
                        // the image tag (see container arm).
                        spawned_container_image = k8s.pod_image_digest(&handle.id).await.ok();
                        register_spawned_agent(
                            state,
                            &agent,
                            handle.clone(),
                            SpawnBackend::Kubernetes(k8s),
                            &resolved_target_config,
                        )
                        .await;
                    }
                    Err(e) => {
                        spawned_pid = None;
                        spawned_container_id = None;
                        spawned_container_image = None;
                        tracing::warn!(
                            agent_id = %agent.id,
                            "kubernetes pod spawn failed (best-effort): {e}"
                        );
                    }
                }
            }
            _ => {
                // agent-runtime.md §3 fallback chain, final link: local
                // container auto-detection (Docker or Podman). Only when no
                // container runtime is available do we spawn a bare local
                // process — the e2e flow relies on this (GYRE_AGENT_COMMAND).
                //
                // An EXPLICITLY configured GYRE_AGENT_COMMAND wins over
                // auto-detection: a deployment that names the agent command
                // has made a runtime choice (e.g. a host-installed agent
                // runner), and silently preferring a locally-installed
                // Docker whose image may not exist would ignore it.
                let explicit_command = std::env::var("GYRE_AGENT_COMMAND").ok();
                let auto_container = if explicit_command.is_some() {
                    None
                } else {
                    gyre_adapters::compute::container::Runtime::detect()
                        .await
                        .ok()
                };
                match auto_container {
                    Some(runtime) => {
                        let image = std::env::var("GYRE_AGENT_IMAGE")
                            .unwrap_or_else(|_| "gyre-agent:latest".to_string());
                        let ct = gyre_adapters::compute::ContainerTarget::new(image.clone())
                            .with_runtime(runtime);
                        match gyre_ports::ComputeTarget::spawn_process(&ct, &spawn_config).await {
                            Ok(handle) => {
                                spawned_pid = handle.pid;
                                launched = true;
                                spawned_container_id = Some(handle.id.clone());
                                // Digest, not image name (see container arm).
                                spawned_container_image = ct.image_digest().await.ok();
                                register_spawned_agent(
                                    state,
                                    &agent,
                                    handle.clone(),
                                    SpawnBackend::Container(ct),
                                    &resolved_target_config,
                                )
                                .await;
                            }
                            Err(e) => {
                                spawned_pid = None;
                                spawned_container_id = None;
                                spawned_container_image = None;
                                tracing::warn!(
                                    agent_id = %agent.id,
                                    "auto-detected container spawn failed (best-effort): {e}"
                                );
                            }
                        }
                    }
                    None => {
                        // No container runtime — bare local process (e2e /
                        // dev environments with GYRE_AGENT_COMMAND).
                        let local = gyre_adapters::compute::LocalTarget;
                        match gyre_ports::ComputeTarget::spawn_process(&local, &spawn_config).await
                        {
                            Ok(handle) => {
                                spawned_pid = handle.pid;
                                launched = true;
                                spawned_container_id = None;
                                spawned_container_image = None;
                                register_spawned_agent(
                                    state,
                                    &agent,
                                    handle.clone(),
                                    SpawnBackend::Local,
                                    &resolved_target_config,
                                )
                                .await;
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
        }
    }

    AgentLaunchResult {
        launched,
        pid: spawned_pid,
        container_id: spawned_container_id,
        container_image: spawned_container_image,
        compute_target_label,
    }
}

// ── Spawned-agent registration (agent-runtime.md §3) ─────────────────────────

/// The concrete compute backend a running agent was spawned on.
///
/// Kept per-agent so lifecycle operations (admin kill, stale detection,
/// exit monitoring) reach the agent on the backend that owns it: a
/// container ID must go to `docker rm --force`, a Pod name to
/// `kubectl delete pod`, a remote container to SSH — a pid-based
/// [`gyre_adapters::compute::LocalTarget`] kill is a silent no-op for all
/// of those (it only works when `handle.pid` is set, which container and
/// Pod handles never are).
pub enum SpawnBackend {
    Container(gyre_adapters::compute::ContainerTarget),
    /// SSH compute target running `docker run` on the remote host
    /// (agent-runtime.md §3 Supported Backends — SSH, spec default).
    SshDocker(gyre_adapters::compute::SshDockerTarget),
    /// Legacy bare remote-process SSH mode (`container_mode: false`).
    SshBare(gyre_adapters::compute::SshTarget),
    Kubernetes(gyre_adapters::compute::KubernetesTarget),
    Local,
}

impl SpawnBackend {
    fn target_type(&self) -> &'static str {
        match self {
            SpawnBackend::Container(_) => "container",
            SpawnBackend::SshDocker(_) | SpawnBackend::SshBare(_) => "ssh",
            SpawnBackend::Kubernetes(_) => "kubernetes",
            SpawnBackend::Local => "local",
        }
    }
}

#[async_trait::async_trait]
impl gyre_ports::ComputeTarget for SpawnBackend {
    fn name(&self) -> &str {
        match self {
            SpawnBackend::Container(t) => t.name(),
            SpawnBackend::SshDocker(t) => t.name(),
            SpawnBackend::SshBare(t) => t.name(),
            SpawnBackend::Kubernetes(t) => t.name(),
            SpawnBackend::Local => gyre_adapters::compute::LocalTarget.name(),
        }
    }

    fn target_type(&self) -> &'static str {
        self.target_type()
    }

    async fn spawn_process(
        &self,
        _config: &gyre_ports::SpawnConfig,
    ) -> anyhow::Result<gyre_ports::ProcessHandle> {
        // Registration-only handle: spawning already happened before the
        // backend was wrapped here. Reaching this is a programming error.
        Err(anyhow::anyhow!(
            "SpawnBackend is a lifecycle handle; call the concrete target to spawn"
        ))
    }

    async fn kill_process(&self, handle: &gyre_ports::ProcessHandle) -> anyhow::Result<()> {
        match self {
            SpawnBackend::Container(t) => t.kill_process(handle).await,
            SpawnBackend::SshDocker(t) => t.kill_process(handle).await,
            SpawnBackend::SshBare(t) => t.kill_process(handle).await,
            SpawnBackend::Kubernetes(t) => t.kill_process(handle).await,
            SpawnBackend::Local => {
                gyre_adapters::compute::LocalTarget
                    .kill_process(handle)
                    .await
            }
        }
    }

    async fn is_alive(&self, handle: &gyre_ports::ProcessHandle) -> anyhow::Result<bool> {
        match self {
            SpawnBackend::Container(t) => t.is_alive(handle).await,
            SpawnBackend::SshDocker(t) => t.is_alive(handle).await,
            SpawnBackend::SshBare(t) => t.is_alive(handle).await,
            SpawnBackend::Kubernetes(t) => t.is_alive(handle).await,
            SpawnBackend::Local => gyre_adapters::compute::LocalTarget.is_alive(handle).await,
        }
    }
}

/// How the exit monitor probes the agent's backend, and how long it waits
/// between probes. Split out so tests can drive one iteration without a
/// real sleep.
const EXIT_MONITOR_POLL_SECS: u64 = 5;

/// Register a freshly spawned agent for lifecycle management:
///
/// 1. Records the process handle in `state.process_registry` (admin kill,
///    stale detection, /agents/{id} status display).
/// 2. Records the backend that owns the handle in
///    `state.spawned_backends` so `kill_process` / `is_alive` reach the
///    right orchestrator instead of assuming `LocalTarget` (which is a
///    no-op for container/Pod/remote-container handles).
/// 3. Starts a background monitor that polls the backend until the agent
///    exits, then updates the agent record (Active → Idle), captures the
///    container exit audit (M19.3), and emits the container_stopped audit
///    event (M23).
///
/// This is the single registration path for every backend dispatch arm in
/// [`spawn_agent_core`]; before the task-117 consolidation each arm
/// hand-rolled its own registry insert + monitor with subtly different
/// cleanup (the container arm was the only one capturing exit audits).
async fn register_spawned_agent(
    state: &Arc<AppState>,
    agent: &Agent,
    handle: gyre_ports::ProcessHandle,
    backend: SpawnBackend,
    resolved_target_config: &Option<super::compute::ComputeTargetConfig>,
) {
    let agent_id_str = agent.id.to_string();
    state
        .process_registry
        .lock()
        .await
        .insert(agent_id_str.clone(), handle.clone());
    state
        .spawned_backends
        .lock()
        .await
        .insert(agent_id_str.clone(), std::sync::Arc::new(backend));

    // Background monitor: watch for process/container/Pod exit and update
    // agent status. Cloning the Arc'd state keeps the loop running after
    // the spawn response returns.
    let state_mon = std::sync::Arc::clone(state);
    tokio::spawn(async move {
        monitor_spawned_agent_exit(state_mon, agent_id_str).await;
    });
    let _ = resolved_target_config; // reserved for future per-target policy
}

/// One background task per spawned agent: poll the backend that owns its
/// handle until it reports not-alive, then run exit bookkeeping.
async fn monitor_spawned_agent_exit(state: Arc<AppState>, agent_id: String) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(EXIT_MONITOR_POLL_SECS)).await;

        let handle = {
            let registry = state.process_registry.lock().await;
            match registry.get(&agent_id) {
                Some(h) => h.clone(),
                // Already unregistered (killed, or another monitor iteration
                // finished first) — nothing left to watch.
                None => break,
            }
        };

        // Probe the backend that owns this handle. Clone the Arc out and
        // DROP the lock before awaiting: an SSH probe can block for the
        // TCP connect timeout, and holding the lock across it would stall
        // admin kills and every new spawn registration.
        let backend = {
            let backends = state.spawned_backends.lock().await;
            match backends.get(&agent_id) {
                Some(b) => std::sync::Arc::clone(b),
                // Already unregistered (killed, or another monitor iteration
                // finished first) — nothing left to watch.
                None => break,
            }
        };
        let alive = gyre_ports::ComputeTarget::is_alive(backend.as_ref(), &handle)
            .await
            .unwrap_or(false);

        if !alive {
            state.process_registry.lock().await.remove(&agent_id);
            state.spawned_backends.lock().await.remove(&agent_id);

            // M19.3: Update the audit record on container exit. No-op when
            // no audit record exists (local/SSH-bare spawns) —
            // `capture_exit_audit` returns early on missing records.
            container_audit::capture_exit_audit(state.container_audits.as_ref(), &agent_id).await;

            // M23: Emit container_stopped audit event (best-effort).
            {
                let audit_rec = state
                    .container_audits
                    .find_by_agent_id(&agent_id)
                    .await
                    .ok()
                    .flatten();
                let exit_code = audit_rec.as_ref().and_then(|r| r.exit_code);
                let ctx = crate::container_audit::AuditCtx {
                    audit: state.audit.as_ref(),
                    broadcast_tx: &state.audit_broadcast_tx,
                };
                let container_id_for_evt = audit_rec.map(|r| r.container_id).unwrap_or_default();
                crate::container_audit::emit_stopped(
                    &ctx,
                    &agent_id,
                    &container_id_for_evt,
                    exit_code,
                )
                .await;
            }

            if let Ok(Some(mut a)) = state.agents.find_by_id(&Id::new(&agent_id)).await {
                if a.status == AgentStatus::Active {
                    let _ = a.transition_status(AgentStatus::Idle);
                    let _ = state.agents.update(&a).await;
                    // Platform Model §7: the run's process is gone — revoke its
                    // task-scoped ephemeral secrets (teardown; logs `secret.revoked`).
                    crate::secret_rotation::revoke_agent_task_secrets(&state, &a).await;
                }
            }
            break;
        }
    }
}

/// POST /api/v1/agents/spawn
///
/// Orchestrated agent provisioning in one call (see spawn_agent_core for
/// MCP tool gyre_spawn_worker shares them.
pub async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<SpawnAgentRequest>,
) -> Result<(StatusCode, Json<SpawnAgentResponse>), ApiError> {
    let resp = spawn_agent_core(&state, req, &auth, "POST /api/v1/agents/spawn").await?;
    Ok((StatusCode::CREATED, Json(resp)))
}

/// Create the completion MR for an agent (agent-runtime.md §1 Phase 6 step 1).
///
/// Shared by the REST `POST /agents/:id/complete` handler and the MCP
/// `gyre_agent_complete` tool so both surfaces produce the same MR: the
/// agent's branch → the repo's target branch, spec_ref propagated from the
/// task, jj work exported to git before the diff stats are computed, and
/// the task transitioned to Review.
///
/// Idempotent: when a non-closed MR already exists for this agent+branch
/// (e.g. a Ralph-loop iteration that re-completed), returns it with
/// `created = false` instead of duplicating it.
pub(crate) async fn create_mr_for_completion(
    state: &Arc<AppState>,
    agent: &Agent,
    branch: &str,
    title: &str,
    target_branch: &str,
    conversation_sha: Option<&str>,
) -> Result<(MergeRequest, bool), ApiError> {
    // The agent's worktree: repository link, branch, and jj export target.
    let worktrees = state.worktrees.find_by_agent(&agent.id).await?;
    let repo_id = worktrees
        .first()
        .map(|wt| wt.repository_id.clone())
        .ok_or_else(|| ApiError::InvalidInput("agent has no associated worktree".to_string()))?;

    // Idempotent complete: if an MR already exists for this agent+branch, return 202.
    {
        let existing_mrs = state.merge_requests.list_by_repo(&repo_id).await?;
        let found = existing_mrs.into_iter().find(|m| {
            m.source_branch == branch
                && m.author_agent_id.as_ref() == Some(&agent.id)
                && m.status != gyre_domain::MrStatus::Closed
        });
        if let Some(existing) = found {
            return Ok((existing, false));
        }
    }

    let now = now_secs();

    // Create MergeRequest
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id,
        title.to_string(),
        branch,
        target_branch,
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

    // jj completion path (source-control.md §4): the agent's in-progress
    // change must become visible to git-backed consumers (diff,
    // can_merge, merge_branches) BEFORE the MR's stats are computed from
    // those git refs (R19 F2: stats read the bare repo's refs, so a
    // post-create export left a fresh MR with empty/stale diff_stats and
    // a pre-export conflict verdict). In the workspace layout, agent work
    // lives in jj's object database — `jj git export` pushes it into the
    // bare git repo's refs, and a bookmark on the source branch makes the
    // branch tip resolvable by name.
    if let Some(wt) = worktrees.first() {
        if std::path::Path::new(&wt.path).join(".jj").exists() {
            // Bookmark SET, not create (R19 F1): the spawn flow
            // pre-creates the source branch in the bare repo and jj's
            // git import materializes it as a local bookmark, so `jj
            // bookmark create` fails with "Bookmark already exists" and
            // the export would push the bookmark's stale spawn-time
            // position — the agent's work at @ would never reach the bare
            // repo and the MR would diff and merge an unchanged branch.
            // `set` moves the existing bookmark onto @.
            if let Err(e) = state
                .jj_ops
                .jj_bookmark_set(&wt.path, &mr.source_branch, "@")
                .await
            {
                tracing::warn!(agent_id = %agent.id, "jj bookmark set failed: {e}");
            }
            if let Err(e) = state.jj_ops.jj_git_export(&wt.path).await {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "jj git export failed — agent work may be invisible to merge checks"
                );
            } else {
                tracing::info!(
                    agent_id = %agent.id,
                    branch = %mr.source_branch,
                    "jj work exported to git on complete"
                );
            }
        } else if std::path::Path::new(&wt.path).exists() {
            // Legacy colocated layout (pre-workspace): bookmark best-effort.
            // `set` here too — the colocated repo's branches are equally
            // likely to pre-exist the completion (R19 F1).
            if let Err(e) = state
                .jj_ops
                .jj_bookmark_set(&wt.path, &mr.source_branch, "@")
                .await
            {
                tracing::debug!(agent_id = %agent.id, "jj bookmark skipped: {e}");
            }
        }
    }

    // Compute diff stats + conflict detection (like create_mr does) —
    // AFTER the jj export above so the stats reflect the agent's actual
    // completed work (R19 F2).
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

            // meta-spec-reconciliation.md §7 auto_merge: when the workspace's
            // rollout policy sets auto_merge, a reconciliation MR merges
            // automatically once its gates pass — enqueue it into the merge
            // queue here (gate passage is still required; the merge processor
            // merges only after gates pass). Default (auto_merge: false)
            // leaves the MR for a human/agent-triggered enqueue.
            crate::reconciliation::maybe_auto_enqueue_reconciliation_mr(&state, &mr, &task).await;
        }
    }

    // Store conversation SHA in KV for merge attestation (HSI §5).
    // The merge processor looks up `conv_sha:{agent_id}` in the `agent_provenance` bucket.
    if let Some(sha) = conversation_sha {
        let kv_key = format!("conv_sha:{}", agent.id);
        let _ = state
            .kv_store
            .kv_set("agent_provenance", &kv_key, sha.to_string())
            .await;
    }
    Ok((mr, true))
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

    let (mr, created) = create_mr_for_completion(
        &state,
        &agent,
        &req.branch,
        &req.title,
        &req.target_branch,
        req.conversation_sha.as_deref(),
    )
    .await?;
    if !created {
        return Ok((StatusCode::ACCEPTED, Json(MrResponse::from(mr))));
    }
    let now = now_secs();

    // Transition agent to Idle
    let _ = agent.transition_status(AgentStatus::Idle);
    state.agents.update(&agent).await?;

    // Agent lifecycle audit events (observability.md §Audit Event Types).
    crate::agent_audit::emit_completed(
        &state,
        &agent,
        agent.current_task_id.as_ref(),
        &mr.id,
        &mr.source_branch,
    )
    .await;

    // Revoke the agent's token — completed agents must not continue to authenticate (N-1).
    // Revocation is audited as an AgentTokenRevoked event carrying the token's jti.
    crate::agent_audit::revoke_agent_token(&state, &agent, "agent_completed").await;

    // HSI §4: Clean up interrogation ABAC policies on completion.
    cleanup_interrogation_policies(&state, &id).await;
    // Also remove any stored conversation context for this agent.
    let _ = state.kv_store.kv_remove("interrogation_context", &id).await;

    // Platform Model §7: revoke task-scoped ephemeral secrets on completion
    // ("Secrets access revoked" at teardown) — deletes Task-scoped secrets
    // for the agent's task and logs `secret.revoked` per secret. Mirrors the
    // MCP gyre_agent_complete teardown.
    crate::secret_rotation::revoke_agent_task_secrets(&state, &agent).await;
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
            "mr",
            None,
        )
        .await;
    }

    // TASK-022: If this agent's task was a cascade test, report the result.
    // Agent completion means the cascade test passed (tests ran successfully).
    if let Some(task_id) = &agent.current_task_id {
        if let Ok(Some(task)) = state.tasks.find_by_id(task_id).await {
            if task.labels.iter().any(|l| l == "cascade-test") {
                crate::merge_processor::report_cascade_test_result(&state, &task, true, None).await;
            }
        }
    }

    // agent-runtime.md §1 Phase 6: gates run automatically on agent
    // completion. Entering the merge queue triggers gate execution and the
    // Ralph-loop watcher (Phase 7) observes the results.
    crate::ralph_loop::on_agent_mr_created(&state, &agent, &mr).await;

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

    let workspace = state
        .workspaces
        .find_by_id(&agent.workspace_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "agent {} references missing workspace {}",
                agent.id,
                agent.workspace_id
            ))
        })?;

    let now = now_secs();
    let usage = AgentUsage {
        agent_id: agent.id.clone(),
        tokens_input: req.tokens_input,
        tokens_output: req.tokens_output,
        cost_usd: req.cost_usd,
        reported_at: now,
    };

    state.agents.record_usage(&usage).await?;

    // Platform-model.md §Budget Tracking: every usage report appends one
    // per-call budget record and rolls into the workspace + tenant
    // tokens_used_today / cost_today counters (the input check_spawn_budget
    // evaluates). Best-effort — never fails the report.
    super::budget::record_llm_call_usage(
        &state,
        &super::budget::LlmCallUsage {
            tenant_id: workspace.tenant_id.clone(),
            workspace_id: agent.workspace_id.clone(),
            repo_id: agent.repo_id.clone(),
            agent_id: Some(agent.id.clone()),
            task_id: agent.current_task_id.clone(),
            usage_type: "agent_run".to_string(),
            input_tokens: req.tokens_input,
            output_tokens: req.tokens_output,
            cost_usd: req.cost_usd,
            model: workspace
                .llm_model
                .clone()
                .unwrap_or_else(|| crate::llm_helpers::DEFAULT_LLM_MODEL.to_string()),
        },
    )
    .await;

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

    // Platform Model §7: Failed is a terminal run-end (agent-runtime.md §1),
    // so the run's task-scoped ephemeral secrets must not outlive it —
    // revoke them alongside the teardown bookkeeping (review F2).
    crate::secret_rotation::revoke_agent_task_secrets(&state, &agent).await;

    // Notify the spawning user that the agent failed (user-management.md
    // §Notification Entity: AgentFailed — High, needs attention soon).
    // Genuine escalations (agent asking for a human) keep AgentEscalation.
    if let Some(ref spawned_by) = agent.spawned_by {
        crate::notifications::notify_rich(
            state.as_ref(),
            agent.workspace_id.clone(),
            Id::new(spawned_by.clone()),
            gyre_common::NotificationType::AgentFailed,
            format!("Agent '{}' failed and needs attention", agent.name),
            "default",
            Some(
                serde_json::json!({
                    "agent_id": agent.id.as_str(),
                    "agent_name": agent.name,
                })
                .to_string(),
            ),
            Some(agent.id.to_string()),
            agent.repo_id.as_ref().map(|r| r.to_string()),
            "agent",
            None,
        )
        .await;
    }

    // TASK-022: If this agent's task was a cascade test, report failure.
    if let Some(ref task_id) = agent.current_task_id {
        if let Ok(Some(task)) = state.tasks.find_by_id(task_id).await {
            if task.labels.iter().any(|l| l == "cascade-test") {
                crate::merge_processor::report_cascade_test_result(
                    &state,
                    &task,
                    false,
                    Some(&format!(
                        "Agent '{}' failed during cascade testing",
                        agent.name
                    )),
                )
                .await;
            }
        }
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
        // early-return:ok - idempotency guard; key generation lives in
        // bootstrap_agent_keypair, a separate pub(crate) fn the check's
        // fn-boundary regex does not recognize, so its keygen is (wrongly)
        // attributed to this function's span.
        return Ok(StatusCode::OK);
    }

    agent
        .transition_status(AgentStatus::Stopped)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    state.agents.update(&agent).await?;

    // Revoke the agent's token so it can no longer authenticate.
    // Revocation is audited as an AgentTokenRevoked event carrying the token's jti.
    crate::agent_audit::revoke_agent_token(&state, &agent, "agent_stopped").await;

    // Platform Model §7: Stopped is terminal (no exit transitions), and the
    // operator/orchestrator initiated it — the run is over, so revoke the
    // task-scoped ephemeral secrets alongside the token (review F1).
    crate::secret_rotation::revoke_agent_task_secrets(&state, &agent).await;

    // M22.2: Decrement budget active-agent counter.
    let workspace_id = agent.workspace_id.to_string();
    super::budget::decrement_active_agents(&state, &workspace_id).await;

    Ok(StatusCode::OK)
}

// ── Derived Input (Phase 3, §7.4) ─────────────────────────────────────────────

/// Generate an agent's own Ed25519 keypair and store it + a workload
/// `KeyBinding` in KV, so the agent can sign output attestations at push time
/// and act as a spawner (signing DerivedInputs for children) later.
///
/// Shared by worker spawn (via `create_derived_input_for_agent`) and
/// orchestrator spawn (task-093): orchestrators have no task, so they call
/// this directly instead of walking an attestation chain.
///
/// Best-effort: logs and returns on key generation failure.
pub(crate) async fn bootstrap_agent_keypair(state: &crate::AppState, agent_id: &str, now: u64) {
    // crypto-verify:ok — this function generates+signs new keys, not verifying external input.
    // The child agent ALWAYS needs its own keypair to sign output attestations
    // at push time, regardless of whether the spawner has a key (§7.4).
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
    use ring::signature::KeyPair;
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
    // DerivedInput stored by create_derived_input_for_agent (which carries the
    // spawner's KeyBinding).
    if let Ok(kb_json) = serde_json::to_string(&child_kb) {
        let _ = state
            .kv_store
            .kv_set("agent_key_bindings", agent_id, kb_json)
            .await;
    }
}

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

    // Find the leaf attestation (highest chain_depth) to derive from.
    let parent_att = match attestations.iter().max_by_key(|a| a.metadata.chain_depth) {
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

    // ── Step 1: Generate the CHILD agent's keypair (unconditional) ──
    bootstrap_agent_keypair(state, agent_id, now).await;

    // ── Step 2: Load the SPAWNER's (orchestrator's) signing key (§4.1, §4.5) ──
    // The DerivedInput must be signed by the spawner (parent/orchestrator), NOT
    // the child agent being spawned. This proves that a specific orchestrator
    // authorized the delegation. The spawner's key was stored when the spawner
    // itself was spawned (or when the orchestrator first received its key).
    // If the spawner has no key, we skip DerivedInput creation but the child's
    // keypair (stored above) is preserved so the delegation chain can bootstrap.
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
                "spawner has no signing key in KV — skipping DerivedInput creation \
                 (child keypair already stored)"
            );
            return; // early-return:ok — child keypair already generated and stored above
        }
    };
    let spawner_pkcs8 = match base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &spawner_key_b64,
    ) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(spawner_id = %spawner_id, "failed to decode spawner key: {e}");
            return; // early-return:ok — child keypair already generated and stored above
        }
    };
    let spawner_key_pair = match ring::signature::Ed25519KeyPair::from_pkcs8(&spawner_pkcs8) {
        Ok(kp) => kp,
        Err(e) => {
            tracing::warn!(spawner_id = %spawner_id, "failed to parse spawner keypair: {e}");
            return; // early-return:ok — child keypair already generated and stored above
        }
    };

    // Load the spawner's own KeyBinding. Check `agent_key_bindings` first (the
    // canonical location), then fall back to extracting from `agent_derived_inputs`
    // (backward compatibility). If neither exists, the spawner is the root agent —
    // build a KeyBinding from its public key.
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
                    use ring::signature::KeyPair;
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
        // domain-event:ok — §7.7 audit-mode: attestation.created is a structured audit log entry, not a broadcast event
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
    use super::complete_agent;
    use super::create_interrogation_policies_in;
    use super::register_spawned_agent;
    use super::spawn_agent_core;
    use super::CompleteAgentRequest;
    use super::SpawnAgentRequest;
    use super::SpawnBackend;
    use crate::mem::{test_state, test_state_with_jj_ops, test_state_with_secrets};
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_ports::ComputeTarget as _;
    use gyre_ports::GitOpsPort;
    use http::{Request, StatusCode};
    use std::sync::Arc;
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

    /// F6 pin (legacy rows): a `repo.path` persisted BEFORE repos_root
    /// was made absolute-at-rest can still be relative in the store. jj
    /// resolves every PATH ARGUMENT (`--git-repo`, the workspace path)
    /// against the command's cwd — the checkout dir — so a relative path
    /// breaks provisioning exactly like the relative root did. The spawn
    /// path must absolutize `repo.path` before handing it (or paths
    /// derived from it) to any jj/git child. This test drives
    /// `spawn_agent_core` directly (the HTTP layer persists
    /// server-computed absolute paths; only the DB can hold a legacy
    /// relative row).
    #[tokio::test]
    async fn spawn_absolutizes_legacy_relative_repo_path_for_jj_children() {
        let jj = std::sync::Arc::new(crate::mem::ConfigurableJjOps::default());
        let state = test_state_with_jj_ops(jj.clone());

        // A repo row with a LEGACY relative path, as persisted before the
        // F6 fix (repos_root defaulted to "./repos").
        let mut repo = gyre_domain::Repository::new(
            Id::new("repo-f6-legacy"),
            Id::new("ws-1"),
            "legacy-repo",
            "repos/ws-1/legacy-repo.git",
            1000,
        );
        repo.default_branch = "main".to_string();
        state.repos.create(&repo).await.unwrap();

        let mut task = gyre_domain::Task::new(Id::new("task-f6"), "legacy path task", 1000);
        task.task_type = Some(gyre_domain::TaskType::Implementation);
        task.workspace_id = Id::new("ws-1");
        state.tasks.create(&task).await.unwrap();
        let auth = crate::auth::AuthenticatedAgent {
            agent_id: "spawner".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let req = crate::api::spawn::SpawnAgentRequest {
            name: "f6-worker".to_string(),
            repo_id: "repo-f6-legacy".to_string(),
            task_id: "task-f6".to_string(),
            branch: "feat/f6".to_string(),
            parent_id: None,
            compute_target_id: None,
            persona_slug: None,
            disconnected_behavior: None,
            loop_config: None,
            agent_type: None,
            conversation_sha: None,
        };
        let _resp = spawn_agent_core(&state, req, &auth, "spawn_agent_core (direct test drive)")
            .await
            .expect("spawn must succeed");

        // Every path handed to the jj children is absolute: the shared
        // checkout init records "{checkout}:{git_repo_path}" and the
        // workspace add records "{name}:{workspace_path}".
        let init_calls = jj.main_checkout_init_calls.lock().clone();
        assert_eq!(init_calls.len(), 1, "one checkout init, got {init_calls:?}");
        let (checkout, git_repo) = init_calls[0].split_once(':').expect("checkout:git");
        assert!(
            std::path::Path::new(checkout).is_absolute(),
            "jj-main checkout path must be absolute, got {checkout:?}"
        );
        assert!(
            std::path::Path::new(git_repo).is_absolute(),
            "--git-repo argument must be absolute, got {git_repo:?}"
        );
        assert!(
            git_repo.ends_with("repos/ws-1/legacy-repo.git"),
            "absolutized path must preserve the repo layout, got {git_repo:?}"
        );

        let add_calls = jj.workspace_add_calls.lock().clone();
        assert_eq!(add_calls.len(), 1, "one workspace add, got {add_calls:?}");
        let (_, ws_path) = add_calls[0].split_once(':').expect("name:workspace_path");
        assert!(
            std::path::Path::new(ws_path).is_absolute(),
            "workspace path argument must be absolute, got {ws_path:?}"
        );
    }

    /// F9 pin: an agent spawned on a branch that does not yet exist in the
    /// bare git repo — the designed-for common case (the agent entrypoint's
    /// `git checkout -b` fallback, the git-worktree fallback's
    /// `git worktree add -b`, MCP gyre_spawn_worker's orchestrator-chosen
    /// branch) — must still get a jj workspace. `jj workspace add -r
    /// <branch>` resolves the branch against the bare repo's imported git
    /// refs; the spawn path therefore creates the branch from the repo HEAD
    /// BEFORE jj provisioning. Pre-fix, provisioning failed ("Revision
    /// '<branch>' doesn't exist") and the agent silently degraded to a
    /// plain-git worktree that the automatic rebase (source-control.md §4)
    /// cannot operate on. Drives `spawn_agent_core` with the REAL git
    /// adapter against a real bare repo — the bug lives in the git-ref
    /// state, which mem doubles cannot see.
    #[tokio::test]
    async fn spawn_creates_missing_branch_before_jj_workspace_add() {
        let jj = std::sync::Arc::new(crate::mem::ConfigurableJjOps::default());
        let git = std::sync::Arc::new(gyre_adapters::git2_ops::Git2OpsAdapter::new());
        let state = crate::mem::test_state_with_jj_and_git_ops(jj.clone(), git.clone());

        // A real bare repo with one commit on main (HEAD resolves).
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("repo.git");
        let bare_str = bare.to_str().unwrap().to_string();
        git.init_bare(&bare_str).await.unwrap();
        git.create_initial_commit(&bare_str, "main").await.unwrap();

        let mut repo = gyre_domain::Repository::new(
            Id::new("repo-f9"),
            Id::new("ws-1"),
            "f9-repo",
            bare_str.clone(),
            1000,
        );
        repo.default_branch = "main".to_string();
        state.repos.create(&repo).await.unwrap();

        let mut task = gyre_domain::Task::new(Id::new("task-f9"), "f9 branch task", 1000);
        task.task_type = Some(gyre_domain::TaskType::Implementation);
        task.workspace_id = Id::new("ws-1");
        state.tasks.create(&task).await.unwrap();
        let auth = crate::auth::AuthenticatedAgent {
            agent_id: "spawner".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let req = crate::api::spawn::SpawnAgentRequest {
            name: "f9-worker".to_string(),
            repo_id: "repo-f9".to_string(),
            task_id: "task-f9".to_string(),
            branch: "feat/brand-new".to_string(),
            parent_id: None,
            compute_target_id: None,
            persona_slug: None,
            disconnected_behavior: None,
            loop_config: None,
            agent_type: None,
            conversation_sha: None,
        };
        let resp = spawn_agent_core(&state, req, &auth, "spawn_agent_core (direct test drive)")
            .await
            .expect("spawn must succeed");

        // jj provisioning succeeded — exactly one workspace add, ON the
        // spawn branch (pre-fix the branch never existed in git, and the
        // real jj adapter would have failed here).
        let revisions = jj.workspace_add_revisions.lock().clone();
        assert_eq!(
            revisions,
            vec!["feat/brand-new".to_string()],
            "jj workspace add must be called with the spawn branch"
        );

        // The spawn branch now exists in the bare repo, based on HEAD —
        // the same base the git-worktree fallback uses. Pre-fix nothing
        // created it (jj "succeeded" only because the double never
        // checked, and the plain-git fallback was skipped).
        assert!(
            git.branch_exists(&bare_str, "feat/brand-new")
                .await
                .unwrap(),
            "spawn branch must exist in the bare repo before jj workspace add"
        );
        let branches = git.list_branches(&bare_str).await.unwrap();
        let main_tip = branches
            .iter()
            .find(|b| b.name == "main")
            .expect("main branch must exist");
        let new_tip = branches
            .iter()
            .find(|b| b.name == "feat/brand-new")
            .expect("spawn branch must be listed");
        assert_eq!(
            main_tip.head_sha, new_tip.head_sha,
            "spawn branch must be created from the repo HEAD"
        );

        // The agent's recorded worktree is the jj workspace (no plain-git
        // degradation).
        let wts = state
            .worktrees
            .find_by_agent(&Id::new(&resp.agent.id))
            .await
            .unwrap();
        assert_eq!(wts.len(), 1);
        assert!(
            wts[0].path.contains("/workspaces/"),
            "jj-provisioned agent must record the jj workspace path, got {}",
            wts[0].path
        );

        // Review de1eec5d F1: the spawn RESPONSE must report the same
        // path — the jj workspace, not the plain-git worktree directory
        // that is never created when jj provisioning succeeded.
        assert_eq!(
            resp.worktree_path, wts[0].path,
            "response worktree_path must equal the recorded worktree path"
        );
    }

    /// Review de1eec5d F1: `worktree_path` in the spawn response must
    /// name the directory the agent actually works in — the value
    /// persisted to the worktree record. When jj provisioning succeeds
    /// the plain-git worktree is NOT created (the jj workspace is the
    /// agent's working directory), so the historical response path
    /// (`{repo}/worktrees/{branch}`) named a nonexistent directory and
    /// the API was internally inconsistent with GET /repos/:id/worktrees.
    /// Covers both provisioning outcomes: jj workspace when provisioned,
    /// plain-git worktree when jj fails. Fails on the pre-fix behavior
    /// (response always cites the worktrees/ path) for the provisioned
    /// case.
    #[tokio::test]
    async fn spawn_response_worktree_path_matches_recorded_worktree() {
        let git = std::sync::Arc::new(gyre_adapters::git2_ops::Git2OpsAdapter::new());

        // Case 1: jj provisioning succeeds — response and record both
        // name the jj workspace.
        {
            let jj = std::sync::Arc::new(crate::mem::ConfigurableJjOps::default());
            let state = crate::mem::test_state_with_jj_and_git_ops(jj.clone(), git.clone());
            let dir = tempfile::tempdir().unwrap();
            let bare = dir.path().join("repo.git");
            let bare_str = bare.to_str().unwrap().to_string();
            git.init_bare(&bare_str).await.unwrap();
            git.create_initial_commit(&bare_str, "main").await.unwrap();
            let mut repo = gyre_domain::Repository::new(
                Id::new("repo-f1a"),
                Id::new("ws-1"),
                "f1a-repo",
                bare_str.clone(),
                1000,
            );
            repo.default_branch = "main".to_string();
            state.repos.create(&repo).await.unwrap();
            let mut task = gyre_domain::Task::new(Id::new("task-f1a"), "f1a task", 1000);
            task.task_type = Some(gyre_domain::TaskType::Implementation);
            task.workspace_id = Id::new("ws-1");
            state.tasks.create(&task).await.unwrap();
            let auth = crate::auth::AuthenticatedAgent {
                agent_id: "spawner".to_string(),
                user_id: None,
                roles: vec![],
                tenant_id: "default".to_string(),
                jwt_claims: None,
                bearer_token: None,
                deprecated_token_auth: false,
                impersonation: None,
            };
            let req = crate::api::spawn::SpawnAgentRequest {
                name: "f1a-worker".to_string(),
                repo_id: "repo-f1a".to_string(),
                task_id: "task-f1a".to_string(),
                branch: "feat/jj-provisioned".to_string(),
                parent_id: None,
                compute_target_id: None,
                persona_slug: None,
                disconnected_behavior: None,
                loop_config: None,
                agent_type: None,
                conversation_sha: None,
            };
            let resp = spawn_agent_core(&state, req, &auth, "spawn_agent_core (direct test drive)")
                .await
                .expect("spawn must succeed");

            let wts = state
                .worktrees
                .find_by_agent(&Id::new(&resp.agent.id))
                .await
                .unwrap();
            assert_eq!(wts.len(), 1);
            assert_eq!(
                resp.worktree_path, wts[0].path,
                "jj-provisioned: response must report the recorded jj workspace path"
            );
            assert!(
                resp.worktree_path.contains("/workspaces/"),
                "jj-provisioned: response must name the jj workspace, got {}",
                resp.worktree_path
            );
        }

        // Case 2: jj provisioning fails — response and record both name
        // the plain-git worktree (which IS created on this path).
        {
            let jj = std::sync::Arc::new(crate::mem::ConfigurableJjOps::default());
            *jj.main_checkout_init_fails.lock() = true;
            let state = crate::mem::test_state_with_jj_and_git_ops(jj.clone(), git.clone());
            let dir = tempfile::tempdir().unwrap();
            let bare = dir.path().join("repo.git");
            let bare_str = bare.to_str().unwrap().to_string();
            git.init_bare(&bare_str).await.unwrap();
            git.create_initial_commit(&bare_str, "main").await.unwrap();
            let mut repo = gyre_domain::Repository::new(
                Id::new("repo-f1b"),
                Id::new("ws-1"),
                "f1b-repo",
                bare_str.clone(),
                1000,
            );
            repo.default_branch = "main".to_string();
            state.repos.create(&repo).await.unwrap();
            let mut task = gyre_domain::Task::new(Id::new("task-f1b"), "f1b task", 1000);
            task.task_type = Some(gyre_domain::TaskType::Implementation);
            task.workspace_id = Id::new("ws-1");
            state.tasks.create(&task).await.unwrap();
            let auth = crate::auth::AuthenticatedAgent {
                agent_id: "spawner".to_string(),
                user_id: None,
                roles: vec![],
                tenant_id: "default".to_string(),
                jwt_claims: None,
                bearer_token: None,
                deprecated_token_auth: false,
                impersonation: None,
            };
            let req = crate::api::spawn::SpawnAgentRequest {
                name: "f1b-worker".to_string(),
                repo_id: "repo-f1b".to_string(),
                task_id: "task-f1b".to_string(),
                branch: "feat/plain/fallback".to_string(),
                parent_id: None,
                compute_target_id: None,
                persona_slug: None,
                disconnected_behavior: None,
                loop_config: None,
                agent_type: None,
                conversation_sha: None,
            };
            let resp = spawn_agent_core(&state, req, &auth, "spawn_agent_core (direct test drive)")
                .await
                .expect("spawn must succeed");

            let wts = state
                .worktrees
                .find_by_agent(&Id::new(&resp.agent.id))
                .await
                .unwrap();
            assert_eq!(wts.len(), 1);
            assert_eq!(
                resp.worktree_path, wts[0].path,
                "plain-git fallback: response must report the recorded worktree path"
            );
            assert!(
                resp.worktree_path.contains("/worktrees/feat-plain-fallback"),
                "plain-git fallback: response must name the created worktree (branch slashes dashed), got {}",
                resp.worktree_path
            );
            assert!(
                std::path::Path::new(&resp.worktree_path).exists(),
                "plain-git fallback: the reported path must exist on disk, got {}",
                resp.worktree_path
            );
        }
    }

    /// Task-106 R19 F1+F2: on completion of a jj-workspace agent, the
    /// source-branch bookmark must be SET (moved onto @), not created —
    /// the spawn flow pre-creates the branch in the bare repo and jj's
    /// git import materializes it as a local bookmark, so `jj bookmark
    /// create` fails with "Bookmark already exists" and the export
    /// would push the stale spawn-time position, silently losing the
    /// agent's work (F1). And the export must run BEFORE the MR's diff
    /// stats are computed from git refs (F2): the jj double's export
    /// side effect flips the shared flag the git double's `diff` reads,
    /// so a fresh MR carrying non-empty diff_stats proves the ordering.
    /// Fails on both pre-fix behaviors.
    #[tokio::test]
    async fn complete_agent_sets_bookmark_and_exports_before_mr_stats() {
        use std::sync::atomic::Ordering;

        let jj = std::sync::Arc::new(crate::mem::ConfigurableJjOps::default());
        let git = std::sync::Arc::new(crate::mem::ConfigurableGitOps::default());
        let state = crate::mem::test_state_with_jj_and_git_ops(jj.clone(), git.clone());

        // The export's effect on git-visible state, observable at the
        // exact call point: once `jj_git_export` runs, `diff` reports
        // the exported work.
        let diff_flag = git.diff_change_when.clone();
        *jj.git_export_effect.lock() = Some(Box::new(move || {
            diff_flag.store(true, Ordering::SeqCst);
        }));

        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().join("repo.git");
        let mut repo = gyre_domain::Repository::new(
            Id::new("repo-r19"),
            Id::new("ws-1"),
            "r19-repo",
            repo_path.to_str().unwrap().to_string(),
            1000,
        );
        repo.default_branch = "main".to_string();
        state.repos.create(&repo).await.unwrap();

        let mut task = gyre_domain::Task::new(Id::new("task-r19"), "r19 completion task", 1000);
        task.task_type = Some(gyre_domain::TaskType::Implementation);
        task.workspace_id = Id::new("ws-1");
        state.tasks.create(&task).await.unwrap();
        let auth = crate::auth::AuthenticatedAgent {
            agent_id: "spawner".to_string(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".to_string(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let req = SpawnAgentRequest {
            name: "r19-worker".to_string(),
            repo_id: "repo-r19".to_string(),
            task_id: "task-r19".to_string(),
            branch: "feat/r19".to_string(),
            parent_id: None,
            compute_target_id: None,
            persona_slug: None,
            disconnected_behavior: None,
            loop_config: None,
            agent_type: None,
            conversation_sha: None,
        };
        let resp = spawn_agent_core(&state, req, &auth, "spawn_agent_core (direct test drive)")
            .await
            .expect("spawn must succeed");
        let agent_id = resp.agent.id.to_string();

        // The recorded worktree is the jj workspace; give it the `.jj`
        // marker the completion path keys on (the double's provisioning
        // records the path without creating the dir).
        let wts = state
            .worktrees
            .find_by_agent(&Id::new(&agent_id))
            .await
            .unwrap();
        assert_eq!(wts.len(), 1);
        let ws_path = wts[0].path.clone();
        assert!(ws_path.contains("/workspaces/"));
        std::fs::create_dir_all(std::path::Path::new(&ws_path).join(".jj"))
            .expect("create workspace .jj marker");

        let (status, mr_json) = {
            let (code, body) = complete_agent(
                axum::extract::State(state.clone()),
                axum::extract::Path(agent_id.clone()),
                axum::Json(CompleteAgentRequest {
                    branch: "feat/r19".to_string(),
                    title: "R19 feature".to_string(),
                    target_branch: "main".to_string(),
                    conversation_sha: None,
                }),
            )
            .await
            .expect("complete must succeed");
            let mr = serde_json::to_value(&body.0).unwrap();
            (code, mr)
        };
        assert_eq!(status, StatusCode::CREATED);

        // F1: the bookmark was SET onto @ in the agent's workspace — and
        // create was never invoked (pre-fix: create on the imported
        // bookmark, which fails in production with "Bookmark already
        // exists").
        let sets = jj.bookmark_set_calls.lock().clone();
        assert_eq!(
            sets,
            vec![(ws_path.clone(), "feat/r19".to_string(), "@".to_string())],
            "completion must SET the source-branch bookmark onto @"
        );
        assert!(
            jj.bookmark_create_calls.lock().is_empty(),
            "completion must not use bookmark create (F1)"
        );

        // The export ran, in the agent's workspace.
        assert_eq!(jj.git_export_calls.lock().clone(), vec![ws_path.clone()]);

        // F2: the MR's diff stats were computed AFTER the export — they
        // carry the exported work (pre-fix: stats read the pre-export
        // refs and came back empty).
        assert_eq!(
            mr_json["diff_stats"]["files_changed"].as_u64(),
            Some(1),
            "diff_stats must reflect the post-export branch state (F2), got {:?}",
            mr_json["diff_stats"]
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
    async fn spawn_emits_agent_spawned_audit_event() {
        // observability.md §Audit Event Types: a successful spawn must record
        // an AgentSpawned audit event with name/task_id/compute_target/branch/
        // spawned_by detail fields.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "audit spawn task").await;
        let (_, json) = do_spawn(app, &repo_id, &task_id, "feat/audit-spawn").await;
        let agent_id = json["agent"]["id"].as_str().unwrap();

        let events = crate::agent_audit::recorded_events(&state, "agent_spawned").await;
        assert_eq!(events.len(), 1, "exactly one agent_spawned event");
        let ev = &events[0];
        assert_eq!(ev.resource_id.as_deref(), Some(agent_id));
        assert_eq!(ev.agent_id.as_ref().map(|id| id.as_str()), Some(agent_id));
        assert_eq!(ev.detail["name"], "worker-1");
        assert_eq!(ev.detail["task_id"], task_id);
        assert_eq!(ev.detail["compute_target"], "local");
        assert_eq!(ev.detail["branch"], "feat/audit-spawn");
        // Global test token authenticates as "system".
        assert_eq!(ev.detail["spawned_by"], "system");
    }

    #[tokio::test]
    async fn complete_emits_agent_completed_and_token_revoked_audit_events() {
        // observability.md §Audit Event Types: a successful complete must
        // record AgentCompleted (task_id/mr_id/branch) and the token
        // revocation must be audited with the revoked token's jti.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, repo_id) = create_repo(app).await;
        let (app, task_id) = create_task(app, "audit complete task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/audit-complete").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();
        let agent_token = spawn_json["token"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "branch": "feat/audit-complete",
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
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mr_json = body_json(resp).await;
        let mr_id = mr_json["id"].as_str().unwrap().to_string();

        let completed = crate::agent_audit::recorded_events(&state, "agent_completed").await;
        assert_eq!(completed.len(), 1, "exactly one agent_completed event");
        let ev = &completed[0];
        assert_eq!(ev.resource_id.as_deref(), Some(agent_id.as_str()));
        assert_eq!(ev.detail["task_id"], task_id);
        assert_eq!(ev.detail["mr_id"], mr_id);
        assert_eq!(ev.detail["branch"], "feat/audit-complete");

        let revoked = crate::agent_audit::recorded_events(&state, "agent_token_revoked").await;
        assert_eq!(revoked.len(), 1, "exactly one agent_token_revoked event");
        assert_eq!(revoked[0].detail["reason"], "agent_completed");
        let jti = revoked[0].detail["jti"]
            .as_str()
            .unwrap_or_else(|| panic!("jti must be a string: {}", revoked[0].detail));
        assert!(!jti.is_empty(), "revocation must carry the token's jti");
        // The audited jti must match the jti actually embedded in the minted token.
        let minted_jti = state
            .agent_signing_key
            .extract_jti_unverified(&agent_token)
            .expect("minted token must carry a jti");
        assert_eq!(jti, minted_jti);
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
    async fn complete_agent_revokes_task_scoped_secrets() {
        // platform-model.md §7 (task-100 review F1): the REST completion path
        // POST /api/v1/agents/{id}/complete must revoke the completing
        // agent's task-scoped ephemeral secrets, mirroring the MCP
        // gyre_agent_complete teardown. Workspace-scoped secrets and secrets
        // of unrelated tasks must survive.
        use gyre_common::{Id, Secret, SecretScope, SecretType};

        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "ws-rest-teardown").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "rest teardown task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/rest-teardown").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // The workspace is created via the API, so it is owned by the
        // caller's tenant ("default" for the global test token).
        let tenant = "default".to_string();
        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: String| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id,
            secret_type: SecretType::Ephemeral,
            created_by: "system".to_string(),
            created_at: now,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant.clone(),
        };
        // Task-scoped ephemeral secret for the completing agent's task (must
        // be revoked); workspace-scoped and other-task secrets must survive.
        state
            .secrets
            .create(
                &mk(
                    "sec-rest-task",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    task_id.clone(),
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-rest-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    ws_id.clone(),
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-rest-other",
                    "OTHER_TASK",
                    SecretScope::Task,
                    "task-unrelated".to_string(),
                ),
                b"v3",
            )
            .await
            .unwrap();

        let body = serde_json::json!({
            "branch": "feat/rest-teardown",
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
        assert_eq!(resp.status(), StatusCode::CREATED);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-rest-task"), &tenant)
                .await
                .unwrap()
                .is_none(),
            "REST agent completion must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-rest-ws"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive agent completion"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-rest-other"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "other tasks' secrets must survive agent completion"
        );
    }

    #[tokio::test]
    async fn stop_agent_revokes_task_scoped_secrets() {
        // platform-model.md §7 (task-100 review F1): POST /api/v1/agents/:id/stop
        // terminally ends the run (Stopped has no exit transitions) and
        // revokes the agent token — it must also revoke the task-scoped
        // ephemeral secrets, like every other run-end path.
        use gyre_common::{Id, Secret, SecretScope, SecretType};

        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "ws-rest-stop").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "rest stop task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/rest-stop").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tenant = "default".to_string();
        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: String| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id,
            secret_type: SecretType::Ephemeral,
            created_by: "system".to_string(),
            created_at: now,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant.clone(),
        };
        state
            .secrets
            .create(
                &mk(
                    "sec-stop-task",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    task_id.clone(),
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-stop-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    ws_id.clone(),
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-stop-other",
                    "OTHER_TASK",
                    SecretScope::Task,
                    "task-unrelated".to_string(),
                ),
                b"v3",
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/stop"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = state
            .agents
            .find_by_id(&Id::new(&agent_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, gyre_domain::AgentStatus::Stopped);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-stop-task"), &tenant)
                .await
                .unwrap()
                .is_none(),
            "REST agent stop must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-stop-ws"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive agent stop"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-stop-other"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "other tasks' secrets must survive agent stop"
        );
    }

    #[tokio::test]
    async fn fail_agent_revokes_task_scoped_secrets() {
        // platform-model.md §7 (task-100 review F2): POST /api/v1/agents/:id/fail
        // marks the agent Failed — a terminal, non-recoverable run-end — so
        // the run's task-scoped ephemeral secrets must be revoked.
        use gyre_common::{Id, Secret, SecretScope, SecretType};

        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "ws-rest-fail").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "rest fail task").await;
        let (app, spawn_json) = do_spawn(app, &repo_id, &task_id, "feat/rest-fail").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tenant = "default".to_string();
        let mk = |id: &str, name: &str, scope: SecretScope, scope_id: String| Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id,
            secret_type: SecretType::Ephemeral,
            created_by: "system".to_string(),
            created_at: now,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant.clone(),
        };
        state
            .secrets
            .create(
                &mk(
                    "sec-fail-task",
                    "EPHEMERAL_DB",
                    SecretScope::Task,
                    task_id.clone(),
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-fail-ws",
                    "SHARED_KEY",
                    SecretScope::Workspace,
                    ws_id.clone(),
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-fail-other",
                    "OTHER_TASK",
                    SecretScope::Task,
                    "task-unrelated".to_string(),
                ),
                b"v3",
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/fail"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = state
            .agents
            .find_by_id(&Id::new(&agent_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, gyre_domain::AgentStatus::Failed);

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-fail-task"), &tenant)
                .await
                .unwrap()
                .is_none(),
            "REST agent fail must revoke the task-scoped secret"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-fail-ws"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive agent fail"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-fail-other"), &tenant)
                .await
                .unwrap()
                .is_some(),
            "other tasks' secrets must survive agent fail"
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

    // ── Agent usage reporting → budget recording (task-190) ────────────────────

    #[tokio::test]
    async fn agent_usage_report_records_budget_call_and_increments_counters() {
        // task-190 regression: POST /api/v1/agents/:id/usage must (a) append a
        // BudgetCallRecord with usage_type "agent_run" and (b) increment the
        // workspace AND tenant tokens_used_today/cost_today counters, so that
        // GET /api/v1/workspaces/:id/budget reflects the usage and
        // check_spawn_budget has real inputs. With the recording wiring
        // removed, the counters stay frozen at 0 and this test fails.
        let state = crate::mem::test_state();
        let app = crate::api::api_router().with_state(Arc::clone(&state));
        let (app, ws_id) = create_workspace(app, "ws-usage-budget").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "usage budget task").await;
        let (_, spawn_json) = do_spawn(app.clone(), &repo_id, &task_id, "feat/usage-budget").await;
        let agent_id = spawn_json["agent"]["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "tokens_input": 600,
            "tokens_output": 400,
            "cost_usd": 0.12,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/usage"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // (a) Per-call audit record retrievable via list_by_workspace.
        let records = state
            .budget_calls
            .list_by_workspace(&ws_id, 0, 10)
            .await
            .unwrap();
        assert_eq!(
            records.len(),
            1,
            "usage report must append exactly one BudgetCallRecord"
        );
        let r = &records[0];
        assert_eq!(r.usage_type, "agent_run");
        assert_eq!(r.input_tokens, 600);
        assert_eq!(r.output_tokens, 400);
        assert!((r.cost_usd - 0.12).abs() < 1e-9);
        assert_eq!(r.agent_id.as_ref().unwrap().as_str(), agent_id);
        assert_eq!(r.workspace_id.as_str(), ws_id);

        // (b) Workspace + tenant counters incremented.
        let ws_usage = state
            .budget_usages
            .get_usage(&super::super::budget::workspace_key(&ws_id))
            .await
            .unwrap()
            .expect("workspace budget usage must exist after usage report");
        assert_eq!(ws_usage.tokens_used_today, 1000);
        assert!((ws_usage.cost_today - 0.12).abs() < 1e-9);
        let tenant_usage = state
            .budget_usages
            .get_usage(super::super::budget::tenant_key())
            .await
            .unwrap()
            .expect("tenant budget usage must exist after usage report");
        assert_eq!(tenant_usage.tokens_used_today, 1000);
        assert!((tenant_usage.cost_today - 0.12).abs() < 1e-9);

        // GET /api/v1/workspaces/:id/budget reflects the increased usage.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{ws_id}/budget"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["usage"]["tokens_used_today"], 1000);
        assert!((v["usage"]["cost_today"].as_f64().unwrap() - 0.12).abs() < 1e-9);
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

    /// task-117 F2: a request-supplied compute_target_id that belongs to a
    /// DIFFERENT tenant must be rejected with 404 — indistinguishable from a
    /// missing target — instead of binding this spawn to the foreign
    /// tenant's infrastructure/config.
    #[tokio::test]
    async fn spawn_rejects_cross_tenant_compute_target() {
        let state = test_state();
        // Seed a compute target in a DIFFERENT tenant directly in the store
        // (the API path derives tenant from the caller, so it cannot produce it).
        let foreign = gyre_domain::ComputeTargetEntity::new(
            gyre_common::Id::new("ct-foreign"),
            gyre_common::Id::new("other-tenant"),
            "foreign-docker".to_string(),
            gyre_domain::ComputeTargetType::Container,
            100,
        );
        state.compute_targets.create(&foreign).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "workspace-ct-xtenant").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "cross-tenant ct task").await;

        let body = serde_json::json!({
            "name": "xtenant-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/xtenant-ct",
            "compute_target_id": "ct-foreign",
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
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "cross-tenant compute target must be rejected, got {}",
            resp.status()
        );

        // The rejected spawn must not leave an agent behind.
        assert!(
            state
                .agents
                .find_by_name("xtenant-agent")
                .await
                .unwrap()
                .is_none(),
            "rejected spawn must not create an agent"
        );
    }

    /// task-117 F2 (defense in depth): a workspace assignment pointing at a
    /// foreign-tenant target (stale or tampered directly in the store — the
    /// PUT path validates tenant) must also fail the spawn rather than
    /// silently binding the foreign target.
    #[tokio::test]
    async fn spawn_rejects_stale_cross_tenant_workspace_assignment() {
        let state = test_state();
        let foreign = gyre_domain::ComputeTargetEntity::new(
            gyre_common::Id::new("ct-stale-foreign"),
            gyre_common::Id::new("other-tenant"),
            "stale-docker".to_string(),
            gyre_domain::ComputeTargetType::Container,
            100,
        );
        state.compute_targets.create(&foreign).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "workspace-ct-stale").await;
        // Tamper the workspace assignment directly in the store.
        let mut ws = state
            .workspaces
            .find_by_id(&gyre_common::Id::new(&ws_id))
            .await
            .unwrap()
            .expect("workspace exists");
        ws.compute_target_id = Some(gyre_common::Id::new("ct-stale-foreign"));
        state.workspaces.update(&ws).await.unwrap();

        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "stale ct task").await;

        // Spawn WITHOUT an explicit compute_target_id: the workspace
        // assignment resolves, but its tenant check must fail the spawn.
        let body = serde_json::json!({
            "name": "stale-ct-agent",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/stale-ct",
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
            StatusCode::NOT_FOUND,
            "stale cross-tenant workspace assignment must fail the spawn, got {}",
            resp.status()
        );
        assert!(
            state
                .agents
                .find_by_name("stale-ct-agent")
                .await
                .unwrap()
                .is_none(),
            "rejected spawn must not create an agent"
        );
    }

    /// task-117 review repair: the SSH dispatch arm's agent-name
    /// charset/length validation used to run AFTER the agent record, agent
    /// token, git worktree + DB record, and task assignment were persisted
    /// — a rejected name (spaces, shell metacharacters, >63 chars) returned
    /// 400 but left an Active agent, the task InProgress and assigned to
    /// that agent, and a worktree record behind, violating this handler's
    /// compensating-rollback invariant. The check now runs at the top of
    /// spawn_agent_core, beside the duplicate-name check, before any
    /// write.
    #[tokio::test]
    async fn spawn_rejects_unsafe_agent_name_on_ssh_target_without_orphans() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, ws_id) = create_workspace(app, "workspace-ssh-name").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "ssh name task").await;

        // SSH-typed compute target in the caller's tenant (container_mode
        // defaults to true — the spec default since task-117).
        let ct_body = serde_json::json!({
            "name": "ssh-name-target",
            "target_type": "Ssh",
            "config": {"user": "agent", "host": "127.0.0.1", "port": 22},
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&ct_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "ssh compute target create should succeed"
        );
        let ct_json = body_json(resp).await;
        let ct_id = ct_json["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "name": "bad name with space",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/ssh-bad-name",
            "compute_target_id": ct_id,
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
            StatusCode::BAD_REQUEST,
            "unsafe agent name on an SSH compute target must be rejected, got {}",
            resp.status()
        );

        // The rejected spawn must leave no orphaned state behind.
        assert!(
            state
                .agents
                .find_by_name("bad name with space")
                .await
                .unwrap()
                .is_none(),
            "rejected spawn must not create an agent"
        );
        let task = state
            .tasks
            .find_by_id(&gyre_common::Id::new(&task_id))
            .await
            .unwrap()
            .expect("task exists");
        assert!(
            task.assigned_to.is_none(),
            "rejected spawn must not assign the task to an agent"
        );
        assert_eq!(
            task.status,
            gyre_domain::TaskStatus::Backlog,
            "rejected spawn must not advance the task to InProgress"
        );
        let worktrees = state
            .worktrees
            .find_by_repo(&gyre_common::Id::new(&repo_id))
            .await
            .unwrap();
        assert!(
            worktrees.is_empty(),
            "rejected spawn must not leave a worktree record behind"
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

    // ── Backend-aware lifecycle tests (agent-runtime.md §3) ──────────────

    /// `register_spawned_agent` must record BOTH the handle and the backend
    /// that owns it. The backend record is what makes admin kill reach the
    /// right orchestrator (docker rm / kubectl delete / ssh docker rm)
    /// instead of falling through to a pid-based LocalTarget kill, which is
    /// a silent no-op for container and Pod handles (their `pid` is None).
    #[tokio::test]
    async fn register_spawned_agent_records_backend_owning_handle() {
        let state = test_state();
        let agent = gyre_domain::Agent::new(gyre_common::Id::new("agt-backend-1"), "b1", 100);
        let handle = gyre_ports::ProcessHandle {
            id: "fake-container-id".to_string(),
            target_type: "container".to_string(),
            pid: None,
        };
        register_spawned_agent(
            &state,
            &agent,
            handle.clone(),
            SpawnBackend::Container(gyre_adapters::compute::ContainerTarget::new(
                "gyre-agent:latest",
            )),
            &None,
        )
        .await;

        assert!(
            state
                .process_registry
                .lock()
                .await
                .contains_key("agt-backend-1"),
            "handle must be registered for admin kill"
        );
        assert!(
            state
                .spawned_backends
                .lock()
                .await
                .contains_key("agt-backend-1"),
            "backend must be registered so kill routes to docker, not LocalTarget"
        );
        // Unregister so the background monitor's first poll breaks cleanly
        // (it observes the missing entry and exits without touching state).
        state.process_registry.lock().await.remove("agt-backend-1");
        state.spawned_backends.lock().await.remove("agt-backend-1");
    }

    /// End-to-end kill contract: an agent registered with a Local backend
    /// (real spawned process) must actually die when admin-killed. This
    /// fails if admin kill forgets to consult `spawned_backends` or if the
    /// registry wiring drops the handle. Uses a real short-lived process —
    /// no container runtime required.
    #[tokio::test]
    async fn admin_kill_terminates_process_via_registered_backend() {
        let state = test_state();

        // Spawn a real process via the local backend.
        let spawn_cfg = gyre_ports::SpawnConfig {
            name: "kill-test-proc".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let handle = gyre_adapters::compute::LocalTarget
            .spawn_process(&spawn_cfg)
            .await
            .expect("spawning sleep should work on linux");
        let pid = handle.pid.expect("local spawn sets pid");

        // Register agent + backend exactly as spawn_agent_core does.
        let mut agent = gyre_domain::Agent::new(gyre_common::Id::new("agt-kill-1"), "k1", 100);
        agent
            .transition_status(gyre_domain::AgentStatus::Active)
            .unwrap();
        state.agents.create(&agent).await.unwrap();
        register_spawned_agent(&state, &agent, handle, SpawnBackend::Local, &None).await;

        // Admin-kill through the real handler (full router path).
        let app = crate::api::api_router().with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/agt-kill-1/kill")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "admin kill should succeed"
        );

        // The process must actually be dead — the whole point of the
        // backend-aware kill. Give SIGTERM a moment to land.
        let mut dead = false;
        for _ in 0..20 {
            let alive = gyre_ports::ComputeTarget::is_alive(
                &gyre_adapters::compute::LocalTarget,
                &gyre_ports::ProcessHandle {
                    id: pid.to_string(),
                    target_type: "local".to_string(),
                    pid: Some(pid),
                },
            )
            .await
            .unwrap_or(false);
            if !alive {
                dead = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            dead,
            "admin kill must terminate the spawned process (pid {pid})"
        );

        // Registry cleanup happened.
        assert!(!state
            .process_registry
            .lock()
            .await
            .contains_key("agt-kill-1"));
        assert!(!state
            .spawned_backends
            .lock()
            .await
            .contains_key("agt-kill-1"));
    }
    /// TASK-077 (F6): `create_interrogation_policies` must fail closed — if
    /// a policy cannot be created (e.g. duplicate id), the error propagates
    /// instead of warn-and-continue, so an interrogation agent can never run
    /// with only a subset of its restriction policies.
    #[tokio::test]
    async fn create_interrogation_policies_fails_closed_on_duplicate() {
        use std::collections::HashMap;
        use tokio::sync::Mutex;

        /// Policy repo that rejects creating a duplicate id (like the SQL
        /// adapters' unique constraints), used to force the failure path.
        struct DuplicateRejectingPolicyRepo {
            policies: Mutex<HashMap<String, gyre_domain::Policy>>,
        }

        #[async_trait::async_trait]
        impl gyre_ports::PolicyRepository for DuplicateRejectingPolicyRepo {
            async fn create(&self, policy: &gyre_domain::Policy) -> anyhow::Result<()> {
                let mut store = self.policies.lock().await;
                if store.contains_key(&policy.id.to_string()) {
                    anyhow::bail!("policy id already exists: {}", policy.id);
                }
                store.insert(policy.id.to_string(), policy.clone());
                Ok(())
            }
            async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<gyre_domain::Policy>> {
                Ok(self.policies.lock().await.get(id).cloned())
            }
            async fn list(&self) -> anyhow::Result<Vec<gyre_domain::Policy>> {
                Ok(self.policies.lock().await.values().cloned().collect())
            }
            async fn list_by_scope(
                &self,
                _scope: &gyre_domain::PolicyScope,
                _scope_id: Option<&str>,
            ) -> anyhow::Result<Vec<gyre_domain::Policy>> {
                Ok(Vec::new())
            }
            async fn update(&self, _policy: &gyre_domain::Policy) -> anyhow::Result<()> {
                Ok(())
            }
            async fn delete(&self, _id: &str) -> anyhow::Result<()> {
                Ok(())
            }
            async fn delete_by_name_prefix(&self, _prefix: &str) -> anyhow::Result<u64> {
                Ok(0)
            }
            async fn delete_by_name_prefix_and_scope_id(
                &self,
                _prefix: &str,
                _scope_id: &str,
            ) -> anyhow::Result<u64> {
                Ok(0)
            }
            async fn record_decision(
                &self,
                _decision: &gyre_domain::PolicyDecision,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn list_decisions(
                &self,
                _subject_id: Option<&str>,
                _resource_type: Option<&str>,
                _limit: usize,
            ) -> anyhow::Result<Vec<gyre_domain::PolicyDecision>> {
                Ok(Vec::new())
            }
        }

        let failing_repo = DuplicateRejectingPolicyRepo {
            policies: Mutex::new(HashMap::new()),
        };

        // First creation succeeds and persists the three policies.
        let ids = create_interrogation_policies_in(&failing_repo, "agent-f6", 1000)
            .await
            .expect("first creation should succeed");
        assert_eq!(ids.len(), 3, "three interrogation policies expected");

        // Second creation for the same agent collides with the existing
        // policy ids — must return Err, not warn-and-continue.
        let err = create_interrogation_policies_in(&failing_repo, "agent-f6", 1001)
            .await
            .expect_err("duplicate policy creation must fail closed");
        assert!(
            err.to_string().contains("interrogation"),
            "error must name the failed interrogation policy, got: {err}"
        );
    }

    // ── Platform Model §7: scoped secrets delivery (task-097 F2) ──────────────
    //
    // These tests exercise the real spawn path end-to-end: secrets are seeded
    // into the AppState's SecretRepository (the mem adapter), an agent is
    // spawned via POST /api/v1/agents/spawn with a Kubernetes-typed compute
    // target, and the delivered environment is observed in the Pod manifest
    // the real dispatch produces. Observability trick: the target's
    // `kubectl_path` config points at a test-written stub `kubectl` — a
    // process-boundary test double, not a code stub — which captures the
    // `kubectl apply -f -` Pod manifest from stdin into a tempfile we parse
    // back. The manifest is built by the production `build_pod_manifest`
    // from the production-resolved env (GYRE_CRED_* secret delivery), so the
    // assertions prove the same resolution/scoping behavior against real
    // dispatch code, with no docker/kube/cluster dependency. Every other
    // kubectl subcommand fails in the stub (digest lookup is best-effort).
    // Spawn failure is best-effort (201 regardless), so the assertions are on
    // the captured manifest, not just the HTTP status.

    use crate::AppState;
    use gyre_common::{Secret, SecretScope, SecretType};

    fn test_secret(name: &str, scope: SecretScope, scope_id: &str, tenant_id: &str) -> Secret {
        let id = uuid::Uuid::new_v4().to_string();
        Secret {
            id: gyre_common::Id::new(&id),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: SecretType::Static,
            created_by: "test".to_string(),
            created_at: 0,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant_id.to_string(),
        }
    }

    async fn seed_secret(
        state: &Arc<AppState>,
        name: &str,
        scope: SecretScope,
        scope_id: &str,
        tenant_id: &str,
        value: &[u8],
    ) {
        let secret = test_secret(name, scope, scope_id, tenant_id);
        state
            .secrets
            .create(&secret, value)
            .await
            .expect("seeding secret into mem repository must succeed");
    }

    /// Create a Kubernetes-typed compute target whose `kubectl_path` config
    /// points at a test-written stub `kubectl`. The stub captures the Pod
    /// manifest that the REAL kubernetes dispatch applies via stdin
    /// (`kubectl apply -f -`) to `dump_path`; every other kubectl subcommand
    /// exits 1 (digest lookups are best-effort in the spawn path). The
    /// manifest's `spec.containers[0].env` is the environment the agent
    /// workload would receive — real env delivery, no cluster dependency.
    async fn create_env_dump_compute_target(
        app: Router,
        name: &str,
        dump_path: &str,
    ) -> (Router, String) {
        let stub_path = format!("{dump_path}.sh");
        let script = format!(
            "#!/bin/sh\n\
# Stub kubectl: capture `apply -f -` manifests, fail everything else.\n\
if [ \"$1\" = \"apply\" ]; then\n\
  {{ cat; echo; }} > {dump_path}\n\
  exit 0\n\
fi\n\
exit 1\n"
        );
        std::fs::write(&stub_path, script).expect("writing kubectl stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod kubectl stub");
        }
        let body = serde_json::json!({
            "name": name,
            "target_type": "Kubernetes",
            "config": {"kubectl_path": stub_path},
            "is_default": false,
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
            "env-dump compute target create should succeed"
        );
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// LocalTarget spawns with `current_dir(work_dir)`, so the worktree path
    /// must exist on disk — NoopGitOps's create_worktree is a no-op.
    fn create_worktree_dir(state: &Arc<AppState>, workspace_id: &str, repo: &str, branch: &str) {
        let worktree_path = format!(
            "{}/{}/{}.git/worktrees/{}",
            state.repos_root,
            workspace_id,
            repo,
            branch.replace('/', "-")
        );
        std::fs::create_dir_all(&worktree_path).expect("creating worktree dir");
    }

    /// Wait (bounded) for the stub kubectl to capture the applied Pod
    /// manifest. The stub appends a newline after the JSON body, so only a
    /// complete capture (newline-terminated) counts — a partial read can
    /// win the race with the writer.
    async fn wait_for_dump(dump_path: &str) -> Option<String> {
        for _ in 0..250 {
            if let Ok(content) = std::fs::read_to_string(dump_path) {
                if content.is_empty() || !content.ends_with('\n') {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    continue;
                }
                return Some(content);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        None
    }

    /// Parse the Pod manifest captured by the stub kubectl into a
    /// name → value env map (`spec.containers[0].env`). Structured
    /// parsing keeps the assertions exact (name/value PAIRING, not
    /// substring containment).
    fn pod_env_map(dump: &str) -> std::collections::BTreeMap<String, String> {
        let manifest: serde_json::Value =
            serde_json::from_str(dump).expect("captured dump must be the Pod manifest JSON");
        manifest["spec"]["containers"][0]["env"]
            .as_array()
            .expect("pod manifest must carry env entries")
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().expect("env entry name").to_string(),
                    e["value"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn spawn_delivers_scoped_secrets_across_all_scopes() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("env-dump").to_string_lossy().into_owned();

        let (app, ws_id) = create_workspace(app, "secrets-ws").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Secrets delivery task").await;
        let (app, ct_id) = create_env_dump_compute_target(app, "env-dump-all", &dump_path).await;

        // The test caller is Admin on tenant "default"; the workspace's
        // tenant derives from the caller, so all scopes live in "default".
        let tenant = "default";
        seed_secret(
            &state,
            "TENANT_VAR",
            SecretScope::Tenant,
            tenant,
            tenant,
            b"tenant-value",
        )
        .await;
        seed_secret(
            &state,
            "WORKSPACE_VAR",
            SecretScope::Workspace,
            &ws_id,
            tenant,
            b"workspace-value",
        )
        .await;
        seed_secret(
            &state,
            "REPO_VAR",
            SecretScope::Repo,
            &repo_id,
            tenant,
            b"repo-value",
        )
        .await;
        seed_secret(
            &state,
            "TASK_VAR",
            SecretScope::Task,
            &task_id,
            tenant,
            b"task-value",
        )
        .await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/secrets");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/secrets", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        assert_eq!(
            env.get("GYRE_CRED_TENANT_VAR").map(String::as_str),
            Some("tenant-value"),
            "tenant-scoped secret must be delivered:\n{dump}"
        );
        assert_eq!(
            env.get("GYRE_CRED_WORKSPACE_VAR").map(String::as_str),
            Some("workspace-value"),
            "workspace-scoped secret must be delivered:\n{dump}"
        );
        assert_eq!(
            env.get("GYRE_CRED_REPO_VAR").map(String::as_str),
            Some("repo-value"),
            "repo-scoped secret must be delivered:\n{dump}"
        );
        assert_eq!(
            env.get("GYRE_CRED_TASK_VAR").map(String::as_str),
            Some("task-value"),
            "task-scoped secret must be delivered:\n{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_secret_delivery_nearest_scope_wins() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("env-dump").to_string_lossy().into_owned();

        let (app, ws_id) = create_workspace(app, "secrets-ws-nearest").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Nearest scope task").await;
        let (app, ct_id) =
            create_env_dump_compute_target(app, "env-dump-nearest", &dump_path).await;

        let tenant = "default";
        seed_secret(
            &state,
            "SHARED",
            SecretScope::Tenant,
            tenant,
            tenant,
            b"from-tenant",
        )
        .await;
        seed_secret(
            &state,
            "SHARED",
            SecretScope::Repo,
            &repo_id,
            tenant,
            b"from-repo",
        )
        .await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/nearest");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/nearest", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        // GYRE_CRED_PROXY is injected by the platform itself (spawn.rs:699);
        // scope the assertion to repo-secret delivery only.
        assert_eq!(
            env.get("GYRE_CRED_SHARED").map(String::as_str),
            Some("from-repo"),
            "repo (nearer) scope must win over tenant for the same name:\n{dump}"
        );
    }

    #[tokio::test]
    async fn kubernetes_pod_working_dir_is_workspace_not_local_worktree() {
        // agent-runtime.md §3: the Kubernetes backend applies a Pod with no
        // volumes; the agent image entrypoint clones its own checkout into
        // /workspace. The spawn path canonicalizes the SERVER-LOCAL
        // worktree into SpawnConfig.work_dir — forwarding it as the Pod's
        // workingDir makes the container fail to start under CRI (runc
        // chdir ENOENT on a path the image does not contain), so the
        // kubernetes arm must override work_dir to /workspace (mirroring
        // the SSH container arm).
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("wd-dump").to_string_lossy().into_owned();

        let (app, ws_id) = create_workspace(app, "k8s-wd-ws").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "k8s workingDir task").await;
        let (app, ct_id) = create_env_dump_compute_target(app, "wd-dump", &dump_path).await;

        // A real on-disk worktree: without the override this path (not
        // present in the agent image, and mounted by no volume) is what
        // the manifest would carry as workingDir.
        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/k8s-wd");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/k8s-wd", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let manifest: serde_json::Value =
            serde_json::from_str(&dump).expect("captured dump must be the Pod manifest JSON");
        let working_dir = manifest["spec"]["containers"][0]["workingDir"]
            .as_str()
            .unwrap_or("");
        assert_eq!(
            working_dir, "/workspace",
            "Pod workingDir must be /workspace (entrypoint checkout), not the \
             server-local worktree:
{dump}"
        );
        // No volume may reference the server-local worktree either — the
        // manifest as applied must not depend on server-local paths.
        let volumes = manifest["spec"]
            .get("volumes")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true);
        assert!(
            volumes,
            "Pod manifest must not mount server-local volumes:
{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_unresolvable_workspace_skips_secret_resolution() {
        // A repo in a nonexistent workspace ("ws-ghost") is accepted by the
        // create-repo validation (it only rejects `..`/`/`), so the spawn
        // path sees workspace=None. The F3 fix must skip secret resolution
        // (warn) rather than fabricate tenant "default" — a tenant-scoped
        // secret named LEAK in the "default" tenant must NOT leak into the
        // agent env.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("env-dump").to_string_lossy().into_owned();

        let (app, repo_id) = create_repo_in_workspace(app, "ws-ghost").await;
        let (app, task_id) = create_task(app, "Ghost workspace task").await;
        let (app, ct_id) = create_env_dump_compute_target(app, "env-dump-ghost", &dump_path).await;

        // The previously-fabricated tenant identity — this is exactly the
        // secret the old code would have delivered.
        seed_secret(
            &state,
            "LEAK",
            SecretScope::Tenant,
            "default",
            "default",
            b"leaked",
        )
        .await;

        create_worktree_dir(&state, "ws-ghost", "wt-repo", "feat/ghost");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/ghost", &ct_id).await;
        assert_eq!(
            json["agent"]["status"], "active",
            "unresolvable workspace must not fail the spawn"
        );

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        // GYRE_CRED_PROXY is platform-injected (not a secret); assert no
        // secret-sourced env var leaked.
        assert!(
            !env.keys().any(|k| k.starts_with("GYRE_CRED_") && k != "GYRE_CRED_PROXY"),
            "no secret-sourced GYRE_CRED_* may be delivered when the workspace is unresolvable:\n{dump}"
        );
        assert_eq!(
            env.get("GYRE_CRED_PROXY").map(String::as_str),
            Some("http://127.0.0.1:8765"),
            "platform-injected cred proxy must still be delivered:\n{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_non_utf8_secret_skipped_others_delivered() {
        // F4: a non-UTF-8 secret value cannot become an env var; it must be
        // skipped with a warning naming the secret (never the value), while
        // the remaining secrets are still delivered.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("env-dump").to_string_lossy().into_owned();

        let (app, ws_id) = create_workspace(app, "secrets-ws-binary").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Binary secret task").await;
        let (app, ct_id) = create_env_dump_compute_target(app, "env-dump-binary", &dump_path).await;

        let tenant = "default";
        seed_secret(
            &state,
            "BINARY_SECRET",
            SecretScope::Tenant,
            tenant,
            tenant,
            b"\xff\xfe\x00binary",
        )
        .await;
        seed_secret(
            &state,
            "TEXT_SECRET",
            SecretScope::Tenant,
            tenant,
            tenant,
            b"text-value",
        )
        .await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/binary");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/binary", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        assert_eq!(
            env.get("GYRE_CRED_TEXT_SECRET").map(String::as_str),
            Some("text-value"),
            "UTF-8 secret must still be delivered:\n{dump}"
        );
        assert!(
            !env.contains_key("GYRE_CRED_BINARY_SECRET"),
            "non-UTF-8 secret must be skipped, not corrupted and delivered:\n{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_secret_resolve_error_does_not_fail_spawn() {
        // A failing SecretRepository must not block the spawn: the resolution
        // error is logged and the agent starts without GYRE_CRED_* env vars.
        // The mem-backed state keeps everything real except the secrets port,
        // which always errors — the resolve-Err branch of the spawn path.
        struct FailingSecrets;
        #[async_trait::async_trait]
        impl gyre_ports::SecretRepository for FailingSecrets {
            async fn create(
                &self,
                _secret: &gyre_common::Secret,
                _value: &[u8],
            ) -> anyhow::Result<()> {
                anyhow::bail!("failing secrets port")
            }
            async fn get_value(
                &self,
                _id: &gyre_common::Id,
                _tenant_id: &str,
            ) -> anyhow::Result<Option<Vec<u8>>> {
                anyhow::bail!("failing secrets port")
            }
            async fn find_by_id(
                &self,
                _id: &gyre_common::Id,
                _tenant_id: &str,
            ) -> anyhow::Result<Option<gyre_common::Secret>> {
                anyhow::bail!("failing secrets port")
            }
            async fn list_by_scope(
                &self,
                _scope: gyre_common::SecretScope,
                _scope_id: &str,
                _tenant_id: &str,
            ) -> anyhow::Result<Vec<gyre_common::Secret>> {
                anyhow::bail!("failing secrets port")
            }
            async fn delete(&self, _id: &gyre_common::Id, _tenant_id: &str) -> anyhow::Result<()> {
                anyhow::bail!("failing secrets port")
            }
            async fn rotate(
                &self,
                _id: &gyre_common::Id,
                _new_value: &[u8],
                _tenant_id: &str,
            ) -> anyhow::Result<()> {
                anyhow::bail!("failing secrets port")
            }
            async fn resolve_for_agent(
                &self,
                _tenant_id: &str,
                _workspace_id: &str,
                _repo_id: &str,
                _task_id: Option<&str>,
            ) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
                anyhow::bail!("resolve_for_agent failed")
            }
        }

        let state = test_state_with_secrets(Arc::new(FailingSecrets));
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("env-dump").to_string_lossy().into_owned();

        let (app, ws_id) = create_workspace(app, "secrets-ws-fail").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Failing secrets task").await;
        let (app, ct_id) = create_env_dump_compute_target(app, "env-dump-fail", &dump_path).await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/failing");
        let (_, json) = do_spawn_with_target(app, &repo_id, &task_id, "feat/failing", &ct_id).await;
        assert_eq!(
            json["agent"]["status"], "active",
            "secret resolution failure must not fail the spawn"
        );

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        // GYRE_CRED_PROXY is platform-injected (not a secret); assert no
        // secret-sourced env var was delivered.
        assert!(
            !env.keys()
                .any(|k| k.starts_with("GYRE_CRED_") && k != "GYRE_CRED_PROXY"),
            "no secret-sourced GYRE_CRED_* may be delivered when resolution fails:\n{dump}"
        );
    }

    // ── MCP Prompts injection at spawn (platform-model.md §4.3) ───────────────

    /// Env-dump compute-target variant capturing the GYRE_PROMPT_* vars the
    /// spawn handler injects (§4.3 system prompts). Create a
    /// Kubernetes-typed compute target whose `kubectl_path` config points
    /// at a test-written stub `kubectl` (same process-boundary double as
    /// `create_env_dump_compute_target`): the stub captures the Pod
    /// manifest the REAL kubernetes dispatch applies via stdin
    /// (`kubectl apply -f -`) to `dump_path`; every other kubectl
    /// subcommand exits 1. The manifest's `spec.containers[0].env` is the
    /// environment the agent workload would receive, so the §4.3 prompt
    /// delivery is observed against real dispatch code with no cluster
    /// dependency.
    async fn create_prompt_dump_compute_target(
        app: Router,
        name: &str,
        dump_path: &str,
    ) -> (Router, String) {
        let stub_path = format!("{dump_path}.sh");
        let script = format!(
            "#!/bin/sh\n\
# Stub kubectl: capture `apply -f -` manifests, fail everything else.\n\
if [ \"$1\" = \"apply\" ]; then\n\
  {{ cat; echo; }} > {dump_path}\n\
  exit 0\n\
fi\n\
exit 1\n"
        );
        std::fs::write(&stub_path, script).expect("writing kubectl stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod kubectl stub");
        }
        let body = serde_json::json!({
            "name": name,
            "target_type": "Kubernetes",
            "config": {"kubectl_path": stub_path},
            "is_default": false,
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
            "prompt-dump compute target create should succeed"
        );
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// Env-dump variant for the meta-spec prompt delivery (agent-runtime §2):
    /// the script writes the injected `GYRE_META_SPEC_SET_SHA` on the first
    /// line and the raw multi-line `GYRE_META_SPEC_PROMPT` after it, atomically
    /// (temp file + rename) so the reader never observes a partial write.
    /// Create a Kubernetes-typed compute target whose `kubectl_path` config
    /// points at a test-written stub `kubectl` (same process-boundary double
    /// as `create_env_dump_compute_target`): the stub captures the Pod
    /// manifest the REAL kubernetes dispatch applies via stdin
    /// (`kubectl apply -f -`) to `dump_path`; every other kubectl subcommand
    /// exits 1. The manifest's `spec.containers[0].env` is the environment
    /// the agent workload would receive, so `GYRE_META_SPEC_PROMPT` /
    /// `GYRE_META_SPEC_SET_SHA` delivery is observed against real dispatch
    /// code with no cluster dependency.
    async fn create_meta_prompt_dump_compute_target(
        app: Router,
        name: &str,
        dump_path: &str,
    ) -> (Router, String) {
        let stub_path = format!("{dump_path}.sh");
        let script = format!(
            "#!/bin/sh\n\
# Stub kubectl: capture `apply -f -` manifests, fail everything else.\n\
if [ \"$1\" = \"apply\" ]; then\n\
  {{ cat; echo; }} > {dump_path}\n\
  exit 0\n\
fi\n\
exit 1\n"
        );
        std::fs::write(&stub_path, script).expect("writing kubectl stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod kubectl stub");
        }
        let body = serde_json::json!({
            "name": name,
            "target_type": "Kubernetes",
            "config": {"kubectl_path": stub_path},
            "is_default": false,
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
            "meta-prompt dump compute target create should succeed"
        );
        let json = body_json(resp).await;
        (app, json["id"].as_str().unwrap().to_string())
    }

    /// Seed an approved persona at tenant scope (the test caller's tenant).
    async fn seed_spawn_persona(state: &Arc<AppState>, slug: &str, system_prompt: &str) -> String {
        let mut persona = gyre_domain::Persona::new(
            Id::new(format!("persona-{slug}")),
            format!("Persona {slug}"),
            slug.to_string(),
            gyre_domain::PersonaScope::Tenant(Id::new("default")),
            system_prompt.to_string(),
            1_700_000_000,
        );
        persona.approval_status = gyre_domain::PersonaApprovalStatus::Approved;
        persona.approved_by = Some("human-admin".to_string());
        persona.approved_at = Some(1_700_000_000);
        state.personas.create(&persona).await.unwrap();
        persona.id.to_string()
    }

    /// Spawn with a persona_slug, asserting CREATED.
    async fn do_spawn_with_persona(
        app: Router,
        name: &str,
        repo_id: &str,
        task_id: &str,
        branch: &str,
        compute_target_id: Option<&str>,
        persona_slug: &str,
    ) -> (Router, serde_json::Value) {
        let mut body = serde_json::json!({
            "name": name,
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": branch,
            "persona_slug": persona_slug,
        });
        if let Some(ct) = compute_target_id {
            body["compute_target_id"] = serde_json::Value::String(ct.to_string());
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
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;
        (app, json)
    }

    #[tokio::test]
    async fn spawn_injects_all_four_system_prompts_with_persona() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp
            .path()
            .join("prompt-dump")
            .to_string_lossy()
            .into_owned();

        let (app, ws_id) = create_workspace(app, "prompts-ws").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Implement prompt injection").await;
        // Give the task a spec ref + description so the context prompt has
        // real content to render.
        {
            let mut task = state
                .tasks
                .find_by_id(&Id::new(&task_id))
                .await
                .unwrap()
                .expect("task must exist");
            task.spec_path = Some("specs/system/platform-model.md".to_string());
            task.description = Some("Acceptance: all four prompts injected.".to_string());
            state.tasks.update(&task).await.unwrap();
        }
        let (app, ct_id) =
            create_prompt_dump_compute_target(app, "prompt-dump-all", &dump_path).await;

        let persona_id = seed_spawn_persona(
            &state,
            "prompt-worker",
            "You are the prompt-worker persona.",
        )
        .await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/prompts");
        let (_, json) = do_spawn_with_persona(
            app,
            "worker-prompts",
            &repo_id,
            &task_id,
            "feat/prompts",
            Some(&ct_id),
            "prompt-worker",
        )
        .await;

        // The resolved persona is bound to the agent row (§4.3 persona binding).
        assert_eq!(json["agent"]["persona_id"], persona_id);

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        assert_eq!(
            env.get("GYRE_PROMPT_PERSONA").map(String::as_str),
            Some("You are the prompt-worker persona."),
            "persona prompt must be injected verbatim:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_PROTOCOL")
                .map(|p| p.starts_with("# Gyre Agent Protocol"))
                .unwrap_or(false),
            "protocol prompt must be injected:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_PROTOCOL")
                .map(|p| p.contains("Ralph loop"))
                .unwrap_or(false),
            "protocol prompt must carry the Ralph loop definition:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONTEXT")
                .map(|p| p.starts_with("# Agent Context"))
                .unwrap_or(false),
            "context prompt must be injected:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONTEXT")
                .map(|p| p.contains("Title: Implement prompt injection"))
                .unwrap_or(false),
            "context prompt must carry the task title:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONTEXT")
                .map(|p| p.contains("Spec ref: specs/system/platform-model.md"))
                .unwrap_or(false),
            "context prompt must carry the task's spec ref:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONTEXT")
                .map(|p| p.contains("Branch: feat/prompts"))
                .unwrap_or(false),
            "context prompt must carry the worktree branch:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONSTRAINTS")
                .map(|p| p.starts_with("# Agent Constraints"))
                .unwrap_or(false),
            "constraints prompt must be injected:\n{dump}"
        );
        assert!(
            env.get("GYRE_PROMPT_CONSTRAINTS")
                .map(|p| p.contains(&format!("Workspace: {ws_id}")))
                .unwrap_or(false),
            "constraints prompt must carry the workspace scope boundary:\n{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_without_persona_injects_prompts_but_no_persona() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp
            .path()
            .join("prompt-dump")
            .to_string_lossy()
            .into_owned();

        let (app, ws_id) = create_workspace(app, "prompts-ws-nopersona").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Prompts without persona").await;
        let (app, ct_id) =
            create_prompt_dump_compute_target(app, "prompt-dump-nopersona", &dump_path).await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/nopersona");
        let (_, json) =
            do_spawn_with_target(app, &repo_id, &task_id, "feat/nopersona", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");
        assert!(
            json["agent"]
                .get("persona_id")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "no persona may be fabricated when none was requested: {}",
            json["agent"]
        );

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("stub kubectl should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);
        for var in [
            "GYRE_PROMPT_PROTOCOL",
            "GYRE_PROMPT_CONTEXT",
            "GYRE_PROMPT_CONSTRAINTS",
        ] {
            assert!(env.contains_key(var), "{var} must be injected:\n{dump}");
        }
        assert!(
            !env.contains_key("GYRE_PROMPT_PERSONA"),
            "no persona prompt may be injected when no persona is bound:\n{dump}"
        );
    }

    #[tokio::test]
    async fn spawn_persona_slug_fails_closed() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let (app, ws_id) = create_workspace(app, "prompts-ws-failclosed").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Fail-closed persona").await;

        // Unknown slug: rejected.
        let body = serde_json::json!({
            "name": "worker-missing-persona",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/missing-persona",
            "persona_slug": "no-such-persona",
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
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unknown persona slug must reject the spawn"
        );
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("not found in scope chain"),
            "error should name the unresolvable persona: {}",
            json["error"]
        );

        // Unapproved (Pending) slug: rejected — never inject unapproved behavior.
        let persona = gyre_domain::Persona::new(
            Id::new("persona-pending-spawn"),
            "Persona pending-spawn",
            "pending-spawn".to_string(),
            gyre_domain::PersonaScope::Tenant(Id::new("default")),
            "pending persona prompt".to_string(),
            1_700_000_000,
        );
        // approval_status stays Pending (Persona::new default)
        state.personas.create(&persona).await.unwrap();
        let body = serde_json::json!({
            "name": "worker-pending-persona",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": "feat/pending-persona",
            "persona_slug": "pending-spawn",
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
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a Pending persona must reject the spawn"
        );
        let json = body_json(resp).await;
        assert!(
            json["error"].as_str().unwrap().contains("approve"),
            "error should demand approval: {}",
            json["error"]
        );

        // No orphaned agent rows from the rejected spawns.
        let agents = state.agents.list().await.unwrap();
        let rejected: Vec<&gyre_domain::Agent> = agents
            .iter()
            .filter(|a| a.name.starts_with("worker-"))
            .collect();
        assert!(
            rejected.is_empty(),
            "rejected spawns must not leave agent rows behind: {:?}",
            rejected.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
        );
    }

    /// Complement of `spawn_delivers_meta_spec_prompt_to_agent_env`: with
    /// no required meta-specs and no spec bindings, the assembled prompt set
    /// is empty and the response must NOT fabricate a `meta_spec_set_sha`.
    #[tokio::test]
    async fn spawn_omits_meta_spec_set_sha_without_required_meta_specs() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Fresh workspace, no required meta-specs seeded: the field is null.
        let (app, ws_id) = create_workspace(app, "prompts-ws-metasha").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Meta-spec sha").await;
        let (_, resp) =
            try_spawn(app, "worker-metasha-1", &repo_id, &task_id, "feat/metasha").await;
        assert_eq!(resp.status(), StatusCode::CREATED, "spawn should succeed");
        let json = body_json(resp).await;
        assert!(
            json.get("meta_spec_set_sha")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "no meta_spec_set_sha may be fabricated when the assembled set is empty: {json}"
        );
    }

    /// Agent-runtime §2 server-side delivery (task-116 repair): the
    /// `container_env` injection of `GYRE_META_SPEC_PROMPT` /
    /// `GYRE_META_SPEC_SET_SHA` and the `meta_spec_set_sha` spawn-response
    /// field are guarded here end-to-end. The stub-kubectl compute target
    /// observes the Pod manifest of a real spawn with a seeded required
    /// meta-spec (built by the production `build_pod_manifest` from the
    /// production-resolved env); removing the `container_env.insert` block,
    /// the `render_system_prompt` call, or the response field fails this
    /// test while the agent-runner suite alone stays green.
    #[tokio::test]
    async fn spawn_delivers_meta_spec_prompt_to_agent_env() {
        use gyre_domain::meta_spec::{MetaSpecApprovalStatus, MetaSpecKind, MetaSpecScope};
        use sha2::Digest;

        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let temp = tempfile::tempdir().expect("tempdir");
        let dump_path = temp.path().join("meta-dump").to_string_lossy().into_owned();

        // One required, approved, tenant-scoped persona: it must be
        // assembled into the prompt set for every spawn in any workspace.
        let prompt_text = "You are the meta-spec delivery regression persona.";
        let content_hash = {
            let mut h = sha2::Sha256::new();
            h.update(prompt_text.as_bytes());
            hex::encode(h.finalize())
        };
        state
            .meta_specs
            .create(&gyre_domain::MetaSpec {
                id: gyre_common::Id::new("ms-spawn-delivery"),
                kind: MetaSpecKind::Persona,
                name: "spawn-delivery-persona".to_string(),
                scope: MetaSpecScope::Global,
                scope_id: None,
                prompt: prompt_text.to_string(),
                version: 1,
                content_hash,
                required: true,
                approval_status: MetaSpecApprovalStatus::Approved,
                approved_by: Some("system".to_string()),
                approved_content_hash: None,
                approved_at: Some(1),
                created_by: "system".to_string(),
                created_at: 1,
                updated_at: 1,
            })
            .await
            .expect("seeding required meta-spec must succeed");

        let (app, ws_id) = create_workspace(app, "meta-spec-ws").await;
        let (app, repo_id) = create_repo_in_workspace(app, &ws_id).await;
        let (app, task_id) = create_task(app, "Meta-spec delivery task").await;
        let (app, ct_id) =
            create_meta_prompt_dump_compute_target(app, "meta-dump-ct", &dump_path).await;

        create_worktree_dir(&state, &ws_id, "wt-repo", "feat/meta-prompt");
        let (_, json) =
            do_spawn_with_target(app, &repo_id, &task_id, "feat/meta-prompt", &ct_id).await;
        assert_eq!(json["agent"]["status"], "active");

        // The spawn response must carry the real set SHA of the assembled set.
        let resp_sha = json["meta_spec_set_sha"]
            .as_str()
            .expect("meta_spec_set_sha must be present when the prompt set is non-empty")
            .to_string();
        assert_eq!(resp_sha.len(), 64, "set SHA must be a SHA-256 hex digest");
        assert!(
            resp_sha.chars().all(|c| c.is_ascii_hexdigit()),
            "set SHA must be hex: {resp_sha}"
        );

        let dump = wait_for_dump(&dump_path)
            .await
            .expect("kubectl stub should have captured the applied Pod manifest");
        let env = pod_env_map(&dump);

        // The env-var delivery must carry the same set the response attests.
        let env_sha = env
            .get("GYRE_META_SPEC_SET_SHA")
            .expect("pod manifest must carry GYRE_META_SPEC_SET_SHA")
            .to_string();
        assert_eq!(
            env_sha, resp_sha,
            "GYRE_META_SPEC_SET_SHA env var must match the spawn response's meta_spec_set_sha"
        );

        // The rendered system prompt must contain the seeded meta-spec
        // section: kind, name, scope, version, and the prompt content.
        let env_prompt = env
            .get("GYRE_META_SPEC_PROMPT")
            .expect("pod manifest must carry GYRE_META_SPEC_PROMPT")
            .to_string();
        assert!(
            env_prompt.contains("# meta:persona — spawn-delivery-persona (scope: global, v1)"),
            "rendered prompt must contain the seeded persona section header:\n{env_prompt}"
        );
        assert!(
            env_prompt.contains(prompt_text),
            "rendered prompt must contain the seeded persona prompt text:\n{env_prompt}"
        );
    }

    /// Like `do_spawn`, but with an explicit compute_target_id in the request
    /// body so the env-dump target drives the local dispatch.
    async fn do_spawn_with_target(
        app: Router,
        repo_id: &str,
        task_id: &str,
        branch: &str,
        compute_target_id: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({
            "name": "worker-1",
            "repo_id": repo_id,
            "task_id": task_id,
            "branch": branch,
            "compute_target_id": compute_target_id,
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
}
