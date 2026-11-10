/// Budget governance API — GET/PUT workspace budget limits, GET tenant summary.
///
/// Workspaces are keyed by project_id (the current governance boundary).
/// Budget state is persisted via port traits:
///   - `budget_configs` (BudgetRepository): entity_key -> BudgetConfig (limits)
///   - `budget_usages` (BudgetUsageRepository): entity_key -> BudgetUsage (real-time usage)
///
/// Entity keys: `"workspace:{project_id}"` or `"tenant:global"`.
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    Json,
};
use gyre_common::{Id, NotificationType};
use gyre_common::message::{Destination, MessageKind};
use gyre_domain::{Agent, AgentStatus, BudgetCallRecord, BudgetConfig, BudgetEnforcementAction, BudgetUsage};
use serde::{Deserialize, Serialize};

use super::{new_id, now_secs};
use crate::{api::error::ApiError, auth::AuthenticatedAgent, AppState};

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn workspace_key(project_id: &str) -> String {
    format!("workspace:{project_id}")
}

pub fn tenant_key() -> &'static str {
    "tenant:global"
}

fn new_ws_usage(entity_id: &str) -> BudgetUsage {
    BudgetUsage {
        entity_type: "workspace".into(),
        entity_id: gyre_common::Id::new(entity_id.to_string()),
        tokens_used_today: 0,
        cost_today: 0.0,
        active_agents: 0,
        period_start: now_secs(),
    }
}

fn new_tenant_usage() -> BudgetUsage {
    BudgetUsage {
        entity_type: "tenant".into(),
        entity_id: gyre_common::Id::new("global".to_string()),
        tokens_used_today: 0,
        cost_today: 0.0,
        active_agents: 0,
        period_start: now_secs(),
    }
}

// ── Request / response shapes ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct BudgetResponse {
    pub entity_type: String,
    pub entity_id: String,
    pub config: BudgetConfig,
    pub usage: BudgetUsage,
}

#[derive(Debug, Deserialize)]
pub struct SetBudgetRequest {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
    /// Per-level override for the ≥100% enforcement action (snake_case name
    /// of a `BudgetEnforcementAction` variant). `None` = spec default
    /// (80% Warn, 100% GracefulStop).
    pub enforcement_action: Option<BudgetEnforcementAction>,
}

#[derive(Debug, Serialize)]
pub struct TenantBudgetSummary {
    pub tenant_config: BudgetConfig,
    pub tenant_usage: BudgetUsage,
    pub workspaces: Vec<BudgetResponse>,
}

// ── GET /api/v1/workspaces/{id}/budget ────────────────────────────────────────

pub async fn get_workspace_budget(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<BudgetResponse>, ApiError> {
    let key = workspace_key(&id);
    let config = state
        .budget_configs
        .get_config(&key)
        .await
        .map_err(ApiError::Internal)?
        .unwrap_or_default();
    let usage = state
        .budget_usages
        .get_usage(&key)
        .await
        .map_err(ApiError::Internal)?
        .unwrap_or_else(|| new_ws_usage(&id));
    Ok(Json(BudgetResponse {
        entity_type: "workspace".into(),
        entity_id: id,
        config,
        usage,
    }))
}

// ── PUT /api/v1/workspaces/{id}/budget  (Admin only) ─────────────────────────

pub async fn set_workspace_budget(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<SetBudgetRequest>,
) -> Result<Json<BudgetResponse>, ApiError> {
    // Admin-only: workspace budget limits are financial governance controls.
    // Allowing Developer/Agent roles to raise their own limits could exhaust
    // tenant resources (NEW-32).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(
            &state,
            &auth,
            "Admin",
            "PUT /api/v1/workspaces/:id/budget",
        )
        .await;
        return Err(ApiError::Forbidden(
            "only Admin role may update workspace budget limits".to_string(),
        ));
    }
    let new_config = BudgetConfig {
        max_tokens_per_day: req.max_tokens_per_day,
        max_cost_per_day: req.max_cost_per_day,
        max_concurrent_agents: req.max_concurrent_agents,
        max_agent_lifetime_secs: req.max_agent_lifetime_secs,
        enforcement_action: req.enforcement_action,
    };

    // Cascade: workspace limits cannot exceed tenant limits.
    if let Some(tenant) = state
        .budget_configs
        .get_config(tenant_key())
        .await
        .map_err(ApiError::Internal)?
    {
        if let (Some(ws), Some(t)) = (new_config.max_tokens_per_day, tenant.max_tokens_per_day) {
            if ws > t {
                return Err(ApiError::InvalidInput(format!(
                    "workspace max_tokens_per_day ({ws}) exceeds tenant limit ({t})"
                )));
            }
        }
        if let (Some(ws), Some(t)) = (new_config.max_cost_per_day, tenant.max_cost_per_day) {
            if ws > t {
                return Err(ApiError::InvalidInput(format!(
                    "workspace max_cost_per_day ({ws:.4}) exceeds tenant limit ({t:.4})"
                )));
            }
        }
        if let (Some(ws), Some(t)) = (
            new_config.max_concurrent_agents,
            tenant.max_concurrent_agents,
        ) {
            if ws > t {
                return Err(ApiError::InvalidInput(format!(
                    "workspace max_concurrent_agents ({ws}) exceeds tenant limit ({t})"
                )));
            }
        }
    }

    let key = workspace_key(&id);
    state
        .budget_configs
        .set_config(&key, &new_config)
        .await
        .map_err(ApiError::Internal)?;
    let usage = state
        .budget_usages
        .get_usage(&key)
        .await
        .map_err(ApiError::Internal)?
        .unwrap_or_else(|| new_ws_usage(&id));
    Ok(Json(BudgetResponse {
        entity_type: "workspace".into(),
        entity_id: id,
        config: new_config,
        usage,
    }))
}

// ── GET /api/v1/budget/summary  (Admin only) ─────────────────────────────────

/// GET /api/v1/budget/summary — Admin only.
///
/// Returns the full tenant budget picture: all workspace configs and usage.
/// Agents must not read this — it reveals limits and consumption for workspaces
/// other than their own and could inform resource-exhaustion strategies (NEW-40).
pub async fn budget_summary(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> Result<Json<TenantBudgetSummary>, ApiError> {
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(&state, &auth, "Admin", "GET /api/v1/budget/summary").await;
        return Err(ApiError::Forbidden(
            "only Admin role may read tenant budget summary".to_string(),
        ));
    }
    let tenant_config = state
        .budget_configs
        .get_config(tenant_key())
        .await
        .map_err(ApiError::Internal)?
        .unwrap_or_default();
    let tenant_usage = state
        .budget_usages
        .get_usage(tenant_key())
        .await
        .map_err(ApiError::Internal)?
        .unwrap_or_else(new_tenant_usage);

    let all_configs = state
        .budget_configs
        .list_all()
        .await
        .map_err(ApiError::Internal)?;

    let mut workspaces = Vec::new();
    for (key, config) in all_configs {
        if let Some(ws_id) = key.strip_prefix("workspace:") {
            let usage = state
                .budget_usages
                .get_usage(&key)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| new_ws_usage(ws_id));
            workspaces.push(BudgetResponse {
                entity_type: "workspace".into(),
                entity_id: ws_id.to_string(),
                config,
                usage,
            });
        }
    }

    Ok(Json(TenantBudgetSummary {
        tenant_config,
        tenant_usage,
        workspaces,
    }))
}

// ── Internal helpers (called from spawn handler and cost recording) ───────────

/// Check if spawning another agent would exceed workspace/tenant budget limits.
pub async fn check_spawn_budget(state: &AppState, project_id: &str) -> Result<(), String> {
    let key = workspace_key(project_id);

    if let Ok(Some(config)) = state.budget_configs.get_config(&key).await {
        let usage = state
            .budget_usages
            .get_usage(&key)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| new_ws_usage(project_id));

        if let Some(max) = config.max_concurrent_agents {
            if usage.active_agents >= max {
                return Err(format!(
                    "workspace budget exceeded: max_concurrent_agents={max} ({} active)",
                    usage.active_agents
                ));
            }
        }
        if let Some(max_tokens) = config.max_tokens_per_day {
            if usage.tokens_used_today >= max_tokens {
                return Err(format!(
                    "workspace budget exceeded: max_tokens_per_day={max_tokens} (used {} today)",
                    usage.tokens_used_today
                ));
            }
        }
        if let Some(max_cost) = config.max_cost_per_day {
            if usage.cost_today >= max_cost {
                return Err(format!(
                    "workspace budget exceeded: max_cost_per_day=${max_cost:.4} (spent ${:.4})",
                    usage.cost_today
                ));
            }
        }
    }

    // Tenant-level concurrent agent check.
    if let Ok(Some(t_config)) = state.budget_configs.get_config(tenant_key()).await {
        if let Some(max) = t_config.max_concurrent_agents {
            let current = state
                .budget_usages
                .get_usage(tenant_key())
                .await
                .ok()
                .flatten()
                .map(|u| u.active_agents)
                .unwrap_or(0);
            if current >= max {
                return Err(format!(
                    "tenant budget exceeded: max_concurrent_agents={max} ({current} active)"
                ));
            }
        }
    }

    Ok(())
}

/// Increment active-agent counters for a workspace and the tenant.
pub async fn increment_active_agents(state: &AppState, project_id: &str) {
    let ws_key = workspace_key(project_id);
    let now = now_secs();
    let _ = state
        .budget_usages
        .increment_active(&ws_key, "workspace", project_id, now)
        .await;
    let _ = state
        .budget_usages
        .increment_active(tenant_key(), "tenant", "global", now)
        .await;
}

/// Decrement active-agent counters for a workspace and the tenant.
pub async fn decrement_active_agents(state: &AppState, project_id: &str) {
    let ws_key = workspace_key(project_id);
    let _ = state.budget_usages.decrement_active(&ws_key).await;
    let _ = state.budget_usages.decrement_active(tenant_key()).await;
}

/// Add token/cost usage to workspace and tenant budgets.
pub async fn record_budget_usage(state: &AppState, project_id: &str, tokens: u64, cost_usd: f64) {
    let ws_key = workspace_key(project_id);
    let now = now_secs();
    let _ = state
        .budget_usages
        .add_tokens_cost(&ws_key, "workspace", project_id, now, tokens, cost_usd)
        .await;
    let _ = state
        .budget_usages
        .add_tokens_cost(tenant_key(), "tenant", "global", now, tokens, cost_usd)
        .await;
}

/// Per-call LLM usage detail sufficient to append one `BudgetCallRecord`
/// (platform-model.md §Budget Tracking) and roll it into the real-time
/// workspace/tenant counters. Callers supply the scope they resolved from
/// authenticated context — never caller-supplied form fields.
#[derive(Debug, Clone)]
pub struct LlmCallUsage {
    pub tenant_id: Id,
    pub workspace_id: Id,
    /// None for user-initiated LLM queries with no repo context.
    pub repo_id: Option<Id>,
    /// None for user-initiated LLM queries.
    pub agent_id: Option<Id>,
    pub task_id: Option<Id>,
    /// "agent_run" or "llm_query".
    pub usage_type: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub model: String,
}

/// Append one per-call budget audit record and increment the workspace +
/// tenant `tokens_used_today` / `cost_today` counters for it.
///
/// This is the single entry point for LLM usage recording: every
/// agent-initiated report (`POST /api/v1/agents/:id/usage`) and every
/// user-initiated LLM query path (briefing/ask, explorer-views/generate,
/// specs/assist, MCP spec_assist) funnels through here so the per-call audit
/// trail and the aggregate counters can never diverge. After the counters
/// are updated it runs budget enforcement (platform-model.md §5 Enforcement
/// Behavior) against the new utilization.
///
/// Best-effort persistence: a failed append or counter increment is logged
/// and never fails the caller's request (the LLM answer is already produced;
/// usage recording must not discard it).
pub async fn record_llm_call_usage(state: &Arc<AppState>, usage: &LlmCallUsage) {
    let record = BudgetCallRecord {
        id: new_id(),
        tenant_id: usage.tenant_id.clone(),
        workspace_id: usage.workspace_id.clone(),
        repo_id: usage.repo_id.clone(),
        agent_id: usage.agent_id.clone(),
        task_id: usage.task_id.clone(),
        usage_type: usage.usage_type.clone(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cost_usd: usage.cost_usd,
        model: usage.model.clone(),
        timestamp: now_secs(),
    };
    if let Err(e) = state.budget_calls.save(&record).await {
        tracing::warn!(
            workspace_id = %usage.workspace_id,
            usage_type = %usage.usage_type,
            "failed to persist budget call record: {e}"
        );
    }
    record_budget_usage(
        state,
        &usage.workspace_id.to_string(),
        usage.input_tokens.saturating_add(usage.output_tokens),
        usage.cost_usd,
    )
    .await;

    // Turn the recorded numbers into the specced enforcement actions
    // (platform-model.md §5 Enforcement Behavior). Single helper — call
    // sites must not re-implement threshold detection.
    check_and_enforce_budget(state, &usage.workspace_id.to_string()).await;
}

// ── Budget enforcement (platform-model.md §5 Enforcement Behavior) ───────────

/// Utilization fraction at which agents receive a BudgetWarning message
/// and their spawners a BudgetWarning notification.
pub const BUDGET_WARN_THRESHOLD: f64 = 0.80;
/// Utilization fraction at which the exhausted action fires.
pub const BUDGET_EXHAUSTED_THRESHOLD: f64 = 1.00;
/// Production grace period between BudgetExhausted and the kill (seconds).
pub const DEFAULT_BUDGET_GRACE_SECS: u64 = 60;

/// KV namespace for per-period enforcement de-duplication markers.
/// Key = budget entity key (e.g. "workspace:{id}"), value = serialized
/// `EnforcementMarker`. Port-backed so the marker survives restarts.
const BUDGET_ENFORCEMENT_NS: &str = "budget_enforcement";

/// Per-period emission state for one budget entity. Threshold crossings
/// fire at most once per daily period so repeated tool calls don't spam;
/// `period_start` pins the marker to the counters' current daily period —
/// when the midnight reset stamps a new `period_start`, a stale marker is
/// discarded and the thresholds can fire again.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct EnforcementMarker {
    period_start: u64,
    warned: bool,
    exhausted: bool,
}

async fn load_enforcement_marker(state: &AppState, key: &str) -> EnforcementMarker {
    match state.kv_store.kv_get(BUDGET_ENFORCEMENT_NS, key).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => EnforcementMarker::default(),
    }
}

async fn save_enforcement_marker(state: &AppState, key: &str, marker: &EnforcementMarker) {
    let Ok(raw) = serde_json::to_string(marker) else {
        return;
    };
    if let Err(e) = state
        .kv_store
        .kv_set(BUDGET_ENFORCEMENT_NS, key, raw)
        .await
    {
        tracing::warn!(entity = %key, "failed to persist budget enforcement marker: {e}");
    }
}

/// Highest utilization across the configured daily limits, as a fraction of
/// the limit (1.0 = at limit). `None` when no limit is configured (or a
/// limit is zero — nothing to enforce against). Guards divide-by-zero.
fn budget_utilization(config: &BudgetConfig, usage: &BudgetUsage) -> Option<f64> {
    let token_util = config
        .max_tokens_per_day
        .filter(|m| *m > 0)
        .map(|m| usage.tokens_used_today as f64 / m as f64);
    let cost_util = config
        .max_cost_per_day
        .filter(|m| *m > 0.0)
        .map(|m| usage.cost_today / m);
    match (token_util, cost_util) {
        (Some(t), Some(c)) => Some(t.max(c)),
        (t, c) => t.or(c),
    }
}

/// Agents in the workspace that currently hold a live execution slot.
async fn active_workspace_agents(state: &AppState, ws_id: &Id) -> Vec<Agent> {
    state
        .agents
        .list_by_workspace(ws_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.status == AgentStatus::Active)
        .collect()
}

/// Workspace tenant id for notifications. `None` (skip + log) when the
/// workspace cannot be resolved — a fabricated scope identity would
/// re-target the notification and could leak it across tenants.
async fn workspace_tenant_id(state: &AppState, ws_id: &Id) -> Option<String> {
    match state.workspaces.find_by_id(ws_id).await {
        Ok(Some(ws)) => Some(ws.tenant_id.to_string()),
        Ok(None) => {
            tracing::warn!(workspace_id = %ws_id, "budget enforcement: workspace not found; skipping notification");
            None
        }
        Err(e) => {
            tracing::warn!(workspace_id = %ws_id, "budget enforcement: workspace lookup failed: {e}");
            None
        }
    }
}

/// Emit `MessageKind::BudgetWarning` to each active agent in the workspace
/// and a `NotificationType::BudgetWarning` (priority 7) to their spawner(s).
///
/// Each agent gets BOTH a workspace-feed copy and a Directed
/// `Destination::Agent` copy: the agent MCP poll channel
/// (GET /api/v1/agents/:id/messages -> `messages.list_after`) only returns
/// messages addressed `to_type == "agent"`, so a workspace-only emission
/// would never reach the agent the warning is about.
async fn emit_budget_warn(state: &AppState, ws_id: &Id, agents: &[Agent], usage_pct: f64) {
    for agent in agents {
        let payload = serde_json::json!({
            "agent_id": agent.id.to_string(),
            "workspace_id": ws_id.to_string(),
            "usage_pct": usage_pct,
        });
        // Broadcast to the workspace feed.
        state
            .emit_event(
                Some(ws_id.clone()),
                Destination::Workspace(ws_id.clone()),
                MessageKind::BudgetWarning,
                Some(payload.clone()),
            )
            .await;
        // Directed to the agent (MCP notification channel).
        state
            .emit_event(
                Some(ws_id.clone()),
                Destination::Agent(agent.id.clone()),
                MessageKind::BudgetWarning,
                Some(payload),
            )
            .await;
    }
    if agents.is_empty() {
        return;
    }
    let Some(tenant_id) = workspace_tenant_id(state, ws_id).await else {
        return;
    };
    let mut notified = std::collections::HashSet::new();
    for agent in agents {
        let Some(spawner) = &agent.spawned_by else {
            continue;
        };
        if notified.insert(spawner.clone()) {
            crate::notifications::notify(
                state,
                ws_id.clone(),
                Id::new(spawner.clone()),
                NotificationType::BudgetWarning,
                format!(
                    "Workspace budget usage at {:.0}% of the daily limit",
                    usage_pct * 100.0
                ),
                tenant_id.clone(),
            )
            .await;
        }
    }
}

/// Emit `MessageKind::BudgetExhausted` to each active agent in the workspace.
///
/// Directed `Destination::Agent` copies are emitted alongside the workspace
/// broadcast so the agent learns (via its MCP message poll) that it must
/// hand off work within the grace period — a workspace-only emission is
/// invisible to the agent's channel.
async fn emit_budget_exhausted(state: &AppState, ws_id: &Id, agents: &[Agent], grace_secs: u64) {
    for agent in agents {
        let payload = serde_json::json!({
            "agent_id": agent.id.to_string(),
            "workspace_id": ws_id.to_string(),
            "grace_secs": grace_secs,
        });
        // Broadcast to the workspace feed.
        state
            .emit_event(
                Some(ws_id.clone()),
                Destination::Workspace(ws_id.clone()),
                MessageKind::BudgetExhausted,
                Some(payload.clone()),
            )
            .await;
        // Directed to the agent (MCP notification channel).
        state
            .emit_event(
                Some(ws_id.clone()),
                Destination::Agent(agent.id.clone()),
                MessageKind::BudgetExhausted,
                Some(payload),
            )
            .await;
    }
}

/// Create the spec'd reassignment task for a budget-killed agent's task:
/// "Agent hit budget limit on TASK-X, needs reassignment or budget increase".
async fn create_budget_reassignment_task(state: &AppState, agent: &Agent) {
    let Some(task_id) = &agent.current_task_id else {
        return;
    };
    let now = now_secs();
    let id = new_id();
    let mut task = gyre_domain::Task::new(
        id.clone(),
        format!(
            "Agent hit budget limit on {}, needs reassignment or budget increase",
            task_id
        ),
        now,
    );
    task.workspace_id = agent.workspace_id.clone();
    task.repo_id = agent
        .repo_id
        .clone()
        .unwrap_or_else(|| Id::new(""));
    task.parent_task_id = Some(task_id.clone());
    task.priority = gyre_domain::TaskPriority::High;
    task.description = Some(format!(
        "Agent '{}' ({}) was killed after the workspace budget was exhausted. \
         Its in-progress work (worktree, branch, partial MR) is preserved. \
         Reassign this task or increase the workspace budget.",
        agent.name, agent.id
    ));
    if let Err(e) = state.tasks.create(&task).await {
        tracing::warn!(
            task_id = %task_id,
            "budget enforcement: failed to create reassignment task: {e}"
        );
        return;
    }
    state
        .emit_event(
            Some(agent.workspace_id.clone()),
            Destination::Workspace(agent.workspace_id.clone()),
            MessageKind::TaskCreated,
            Some(serde_json::json!({
                "task_id": id.to_string(),
                "reason": "budget-exhausted-reassignment",
            })),
        )
        .await;
}

/// Kill every still-active agent in the workspace (budget exhausted, grace
/// period elapsed), preserving in-progress work, and create the reassignment
/// task for each killed agent's task. Re-lists agents at kill time so agents
/// that handed off and went Idle during the grace period are spared.
async fn kill_exhausted_agents(state: &AppState, ws_id: &Id) {
    let agents = active_workspace_agents(state, ws_id).await;
    if agents.is_empty() {
        return;
    }
    let tenant_id = workspace_tenant_id(state, ws_id).await;
    for agent in agents {
        // Preserve the worktree/branch/partial MR — the work must survive
        // for reassignment (platform-model.md §5).
        let reason = "killed after budget exhaustion (grace period elapsed)";
        if let Err(e) = super::admin::kill_agent(
            state,
            &agent,
            reason,
            true,
            "budget_enforcement",
            "budget_kill",
        )
        .await
        {
            tracing::warn!(agent_id = %agent.id, "budget enforcement: kill failed: {e}");
            continue;
        }
        create_budget_reassignment_task(state, &agent).await;
        if let (Some(spawner), Some(tenant)) = (&agent.spawned_by, &tenant_id) {
            crate::notifications::notify(
                state,
                ws_id.clone(),
                Id::new(spawner.clone()),
                NotificationType::AgentEscalation,
                format!(
                    "Agent '{}' was killed: workspace budget exhausted",
                    agent.name
                ),
                tenant.clone(),
            )
            .await;
        }
    }
}

/// `QueuePause` action: pause the merge queue of every repo in the workspace
/// until the budget is reviewed (reuses the merge-processor pause mechanism).
async fn pause_workspace_merge_queues(state: &AppState, ws_id: &Id) {
    match state.repos.list_by_workspace(ws_id).await {
        Ok(repos) => {
            for repo in repos {
                crate::merge_processor::pause_merge_queue(
                    state,
                    &repo,
                    "workspace budget exhausted — merge queue paused until budget is reviewed",
                )
                .await;
            }
        }
        Err(e) => tracing::warn!(
            workspace_id = %ws_id,
            "budget enforcement: failed to list workspace repos for queue pause: {e}"
        ),
    }
}

/// Central budget enforcement (platform-model.md §5 Enforcement Behavior).
///
/// Invoked after every usage recording (the single funnel is
/// `record_llm_call_usage`) — never re-implemented at call sites. Computes
/// the workspace's utilization from the real counters and, per daily period:
///
/// - ≥ 80% and < 100% of a limit: `Warn` — `BudgetWarning` message to the
///   active agents + `BudgetWarning` notification to their spawners.
/// - ≥ 100% of a limit: the configured `enforcement_action`, defaulting to
///   `GracefulStop` — `BudgetExhausted` message, then a grace period before
///   still-active agents are killed (work preserved) and a reassignment
///   task is created per killed agent's task. `HardKill` skips the grace;
///   `QueuePause` pauses the workspace merge queues instead of killing.
///
/// Each threshold fires at most once per daily period (persisted marker), so
/// repeated tool calls don't spam.
///
/// Returns the grace-timer join handle when a `GracefulStop` was scheduled
/// (production callers detach it; tests await it with a zeroed grace).
pub async fn check_and_enforce_budget(state: &Arc<AppState>, workspace_id: &str) {
    check_and_enforce_budget_with_grace(state, workspace_id, DEFAULT_BUDGET_GRACE_SECS).await;
}

/// `check_and_enforce_budget` with an injectable grace period — the
/// production default is `DEFAULT_BUDGET_GRACE_SECS` (60s); tests use a
/// near-zero value so the kill path is observable without wall-clock waits.
pub async fn check_and_enforce_budget_with_grace(
    state: &Arc<AppState>,
    workspace_id: &str,
    grace_secs: u64,
) -> Option<tokio::task::JoinHandle<()>> {
    let key = workspace_key(workspace_id);

    let Ok(Some(config)) = state.budget_configs.get_config(&key).await else {
        return None;
    };
    let Ok(Some(usage)) = state.budget_usages.get_usage(&key).await else {
        return None;
    };
    let util = budget_utilization(&config, &usage)?;

    // Per-period de-duplication: discard markers from a previous daily
    // period so the thresholds can fire again after the midnight reset.
    let mut marker = load_enforcement_marker(state, &key).await;
    if marker.period_start != usage.period_start {
        marker = EnforcementMarker {
            period_start: usage.period_start,
            ..Default::default()
        };
    }

    let ws_id = Id::new(workspace_id.to_string());

    if util >= BUDGET_EXHAUSTED_THRESHOLD {
        if marker.exhausted {
            return None;
        }
        marker.exhausted = true;
        save_enforcement_marker(state, &key, &marker).await;
        tracing::warn!(
            workspace_id = %workspace_id,
            utilization = util,
            "workspace budget exhausted (>=100% of a daily limit)"
        );

        let agents = active_workspace_agents(state, &ws_id).await;
        match config
            .enforcement_action
            .unwrap_or(BudgetEnforcementAction::GracefulStop)
        {
            BudgetEnforcementAction::Warn => {
                emit_budget_warn(state, &ws_id, &agents, util).await;
            }
            BudgetEnforcementAction::GracefulStop => {
                emit_budget_exhausted(state, &ws_id, &agents, grace_secs).await;
                let timer_state = Arc::clone(state);
                let timer_ws = ws_id.clone();
                return Some(tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(grace_secs)).await;
                    kill_exhausted_agents(&timer_state, &timer_ws).await;
                }));
            }
            BudgetEnforcementAction::HardKill => {
                emit_budget_exhausted(state, &ws_id, &agents, 0).await;
                kill_exhausted_agents(state, &ws_id).await;
            }
            BudgetEnforcementAction::QueuePause => {
                emit_budget_exhausted(state, &ws_id, &agents, 0).await;
                pause_workspace_merge_queues(state, &ws_id).await;
            }
        }
    } else if util >= BUDGET_WARN_THRESHOLD {
        if marker.warned || marker.exhausted {
            return None;
        }
        marker.warned = true;
        save_enforcement_marker(state, &key, &marker).await;
        tracing::warn!(
            workspace_id = %workspace_id,
            utilization = util,
            "workspace budget warning (>=80% of a daily limit)"
        );

        let agents = active_workspace_agents(state, &ws_id).await;
        emit_budget_warn(state, &ws_id, &agents, util).await;
    }

    None
}

/// Reset daily counters to zero. Called at midnight UTC by background job.
pub async fn reset_daily_counters(state: &AppState) -> anyhow::Result<()> {
    let now = now_secs();
    state.budget_usages.reset_daily_counters(now).await?;
    tracing::info!("Budget daily counters reset at {now}");
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http::StatusCode;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn make_test_app() -> axum::Router {
        crate::build_router(crate::mem::test_state())
    }

    fn json_body(s: &str) -> Body {
        Body::from(s.to_string())
    }

    #[tokio::test]
    async fn get_workspace_budget_returns_defaults() {
        let app = make_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/proj-1/budget")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["entity_type"], "workspace");
        assert_eq!(v["entity_id"], "proj-1");
        assert_eq!(v["usage"]["active_agents"], 0);
        assert_eq!(v["usage"]["tokens_used_today"], 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn developer_cannot_set_workspace_budget() {
        // NEW-32: Developer-role JWT must be rejected with 403 on PUT budget.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-test/budget")
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(json_body(r#"{"max_concurrent_agents":999}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn set_workspace_budget_stores_limits() {
        let app = make_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-1/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(
                        r#"{"max_concurrent_agents":5,"max_tokens_per_day":100000}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["config"]["max_concurrent_agents"], 5);
        assert_eq!(v["config"]["max_tokens_per_day"], 100000);
    }

    #[tokio::test]
    async fn spawn_budget_rejection_at_limit() {
        let state = crate::mem::test_state();
        state
            .budget_configs
            .set_config(
                &super::workspace_key("proj-x"),
                &gyre_domain::BudgetConfig {
                    max_concurrent_agents: Some(0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let result = super::check_spawn_budget(&state, "proj-x").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_concurrent_agents=0"));
    }

    #[tokio::test]
    async fn cascade_validation_rejects_workspace_exceeding_tenant() {
        let state = crate::mem::test_state();
        // Set tenant limit.
        state
            .budget_configs
            .set_config(
                super::tenant_key(),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let app = crate::build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-cascade/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_tokens_per_day":9999999}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_role_cannot_read_budget_summary() {
        // NEW-40 regression: Agent role must be rejected with 403.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        let agent_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "rogue-agent",
                "preferred_username": "rogue-agent",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/budget/summary")
                    .header("authorization", format!("Bearer {agent_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "agent role must not read tenant budget summary (NEW-40)"
        );
    }

    #[tokio::test]
    async fn budget_summary_includes_workspaces() {
        let app = make_test_app();
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/proj-sum/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_concurrent_agents":3}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/budget/summary")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["tenant_config"].is_object());
        let ws_ids: Vec<_> = v["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["entity_id"].as_str().unwrap().to_string())
            .collect();
        assert!(ws_ids.contains(&"proj-sum".to_string()));
    }

    #[tokio::test]
    async fn record_llm_call_usage_increments_workspace_and_tenant_counters() {
        // task-190 regression: recording one LLM call must append a
        // BudgetCallRecord and increment BOTH the workspace and the tenant
        // tokens_used_today/cost_today counters. With the wiring removed
        // (record_budget_usage having zero callers) these stayed frozen at 0.
        let state = crate::mem::test_state();
        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-1"),
                workspace_id: gyre_common::Id::new("ws-budget"),
                repo_id: None,
                agent_id: None,
                task_id: None,
                usage_type: "llm_query".to_string(),
                input_tokens: 700,
                output_tokens: 300,
                cost_usd: 0.05,
                model: "test-model".to_string(),
            },
        )
        .await;

        let ws_usage = state
            .budget_usages
            .get_usage(&super::workspace_key("ws-budget"))
            .await
            .unwrap()
            .expect("workspace usage must exist after recording");
        assert_eq!(ws_usage.tokens_used_today, 1000);
        assert!((ws_usage.cost_today - 0.05).abs() < 1e-9);

        let tenant_usage = state
            .budget_usages
            .get_usage(super::tenant_key())
            .await
            .unwrap()
            .expect("tenant usage must exist after recording");
        assert_eq!(tenant_usage.tokens_used_today, 1000);
        assert!((tenant_usage.cost_today - 0.05).abs() < 1e-9);

        let records = state
            .budget_calls
            .list_by_workspace("ws-budget", 0, 10)
            .await
            .unwrap();
        assert_eq!(records.len(), 1, "one BudgetCallRecord must be appended");
        let r = &records[0];
        assert_eq!(r.usage_type, "llm_query");
        assert_eq!(r.input_tokens, 700);
        assert_eq!(r.output_tokens, 300);
        assert!((r.cost_usd - 0.05).abs() < 1e-9);
        assert_eq!(r.tenant_id.as_str(), "tenant-1");
        assert!(r.agent_id.is_none() && r.task_id.is_none() && r.repo_id.is_none());
    }

    #[tokio::test]
    async fn recorded_usage_makes_token_budget_limit_fire() {
        // task-190 regression: check_spawn_budget's max_tokens_per_day arm
        // can only fire if recorded usage actually reaches the counters.
        // Before the wiring, tokens_used_today was always 0 and this
        // check passed regardless of the limit.
        let state = crate::mem::test_state();
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-limit"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Below the limit: spawn budget check passes.
        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-1"),
                workspace_id: gyre_common::Id::new("ws-limit"),
                repo_id: None,
                agent_id: None,
                task_id: None,
                usage_type: "llm_query".to_string(),
                input_tokens: 400,
                output_tokens: 100,
                cost_usd: 0.0,
                model: "test-model".to_string(),
            },
        )
        .await;
        assert!(
            super::check_spawn_budget(&state, "ws-limit").await.is_ok(),
            "500/1000 tokens used: spawn must be allowed"
        );

        // Past the limit: the counters are real inputs, so the check fires.
        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-1"),
                workspace_id: gyre_common::Id::new("ws-limit"),
                repo_id: None,
                agent_id: None,
                task_id: None,
                usage_type: "llm_query".to_string(),
                input_tokens: 400,
                output_tokens: 200,
                cost_usd: 0.0,
                model: "test-model".to_string(),
            },
        )
        .await;
        let err = super::check_spawn_budget(&state, "ws-limit")
            .await
            .expect_err("1100/1000 tokens used: spawn must be rejected");
        assert!(
            err.contains("max_tokens_per_day"),
            "error must name the exceeded limit, got: {err}"
        );
    }

    // ── Enforcement tests (platform-model.md §5 Enforcement Behavior) ─────

    /// Test fixture: a workspace, a budget config, an in-progress task, and
    /// an Active agent working on it with a recorded spawner. Returns the ids.
    async fn enforcement_fixture(state: &Arc<crate::AppState>, ws: &str, max_tokens: u64) {
        use gyre_domain::{Agent, AgentStatus, Task, TaskStatus, Workspace};

        let ws_id = gyre_common::Id::new(ws);
        state
            .workspaces
            .create(&Workspace::new(
                ws_id.clone(),
                gyre_common::Id::new("tenant-enf"),
                "enf-workspace",
                "enf-workspace",
                0,
            ))
            .await
            .unwrap();
        state
            .budget_configs
            .set_config(
                &super::workspace_key(ws),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(max_tokens),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut task = Task::new(
            gyre_common::Id::new("task-enf-1"),
            "implement feature",
            0,
        );
        task.transition_status(TaskStatus::InProgress).ok();
        state.tasks.create(&task).await.unwrap();

        let mut agent = Agent::new(
            gyre_common::Id::new("agent-enf-1"),
            "worker-enf",
            0,
        );
        agent.transition_status(AgentStatus::Active).ok();
        agent.workspace_id = ws_id;
        agent.spawned_by = Some("spawner-user".to_string());
        agent.current_task_id = Some(gyre_common::Id::new("task-enf-1"));
        state.agents.create(&agent).await.unwrap();
    }

    async fn count_messages(state: &Arc<crate::AppState>, ws: &str, kind: &str) -> usize {
        state
            .messages
            .list_by_workspace(
                &gyre_common::Id::new(ws),
                Some(kind),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap()
            .len()
    }

    /// The agent MCP poll channel (GET /api/v1/agents/:id/messages →
    /// `list_after`) only returns messages addressed `to_type == "agent"`;
    /// workspace-feed copies are invisible to it. Budget warnings must land
    /// here for the agent to act on them.
    async fn agent_inbox(
        state: &Arc<crate::AppState>,
        agent: &str,
    ) -> Vec<gyre_common::message::Message> {
        state
            .messages
            .list_after(&gyre_common::Id::new(agent), 0, None, 100)
            .await
            .unwrap()
    }

    async fn budget_notifications(
        state: &Arc<crate::AppState>,
        ws: &str,
    ) -> Vec<gyre_common::Notification> {
        state
            .notifications
            .list_for_user(
                &gyre_common::Id::new("spawner-user"),
                Some(&gyre_common::Id::new(ws)),
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn warn_at_80pct_emits_message_and_notification_no_kill() {
        // 850/1000 tokens = 85%: must emit a BudgetWarning message + a
        // BudgetWarning notification, and NOT kill the agent.
        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-warn", 1000).await;

        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-enf"),
                workspace_id: gyre_common::Id::new("ws-enf-warn"),
                repo_id: None,
                agent_id: Some(gyre_common::Id::new("agent-enf-1")),
                task_id: Some(gyre_common::Id::new("task-enf-1")),
                usage_type: "agent_run".to_string(),
                input_tokens: 800,
                output_tokens: 50,
                cost_usd: 0.0,
                model: "test-model".to_string(),
            },
        )
        .await;

        assert_eq!(
            count_messages(&state, "ws-enf-warn", "budget_warning").await,
            1,
            "one BudgetWarning message must be emitted at 85%"
        );
        // The agent itself must receive the warning via its MCP poll
        // channel (Directed copy), not just the workspace feed.
        let inbox = agent_inbox(&state, "agent-enf-1").await;
        assert_eq!(
            inbox
                .iter()
                .filter(|m| m.kind.as_str() == "budget_warning")
                .count(),
            1,
            "agent inbox must contain budget_warning, got: {:?}",
            inbox.iter().map(|m| m.kind.as_str()).collect::<Vec<_>>()
        );
        let notifs = budget_notifications(&state, "ws-enf-warn").await;
        assert_eq!(notifs.len(), 1, "spawner must get one BudgetWarning notification");
        assert_eq!(
            notifs[0].notification_type,
            gyre_common::NotificationType::BudgetWarning
        );

        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            agent.status,
            gyre_domain::AgentStatus::Active,
            "warn tier must not kill"
        );

        // De-duplication: another sub-100% recording in the same period
        // must not re-emit the warning.
        super::record_budget_usage(&state, "ws-enf-warn", 50, 0.0).await;
        super::check_and_enforce_budget(&state, "ws-enf-warn").await;
        assert_eq!(
            count_messages(&state, "ws-enf-warn", "budget_warning").await,
            1,
            "warning must not re-fire within the same period"
        );
        assert_eq!(budget_notifications(&state, "ws-enf-warn").await.len(), 1);
        assert_eq!(
            agent_inbox(&state, "agent-enf-1")
                .await
                .iter()
                .filter(|m| m.kind.as_str() == "budget_warning")
                .count(),
            1,
            "agent inbox must not receive a duplicate warning in the same period"
        );
    }

    #[tokio::test]
    async fn below_threshold_emits_neither_message_nor_notification() {
        // 500/1000 tokens = 50%: nothing must fire (over-firing guard).
        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-below", 1000).await;

        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-enf"),
                workspace_id: gyre_common::Id::new("ws-enf-below"),
                repo_id: None,
                agent_id: Some(gyre_common::Id::new("agent-enf-1")),
                task_id: None,
                usage_type: "agent_run".to_string(),
                input_tokens: 450,
                output_tokens: 50,
                cost_usd: 0.0,
                model: "test-model".to_string(),
            },
        )
        .await;

        assert_eq!(count_messages(&state, "ws-enf-below", "budget_warning").await, 0);
        assert_eq!(count_messages(&state, "ws-enf-below", "budget_exhausted").await, 0);
        assert!(budget_notifications(&state, "ws-enf-below").await.is_empty());
        assert!(
            agent_inbox(&state, "agent-enf-1")
                .await
                .iter()
                .all(|m| {
                    m.kind.as_str() != "budget_warning" && m.kind.as_str() != "budget_exhausted"
                }),
            "below-threshold usage must not deliver budget messages to the agent inbox"
        );
        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, gyre_domain::AgentStatus::Active);
    }

    #[tokio::test]
    async fn exhausted_graceful_stop_kills_preserves_work_creates_reassignment() {
        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-kill", 1000).await;

        // The agent holds a budget slot and a worktree — both must be
        // observable after the kill (slot released, worktree record intact).
        super::increment_active_agents(&state, "ws-enf-kill").await;
        let wt = gyre_domain::AgentWorktree::new(
            gyre_common::Id::new("wt-enf-1"),
            gyre_common::Id::new("agent-enf-1"),
            gyre_common::Id::new("repo-enf"),
            Some(gyre_common::Id::new("task-enf-1")),
            "agent-enf-1/task-enf-1",
            "/tmp/worktrees/agent-enf-1",
            0,
        );
        state.worktrees.create(&wt).await.unwrap();

        // Record past 100% (1200/1000) and run enforcement with a zeroed
        // grace so the kill path is observable without wall-clock waits.
        super::record_budget_usage(&state, "ws-enf-kill", 1200, 0.0).await;
        let handle = super::check_and_enforce_budget_with_grace(&state, "ws-enf-kill", 0)
            .await
            .expect("GracefulStop must schedule a grace timer");

        // BudgetExhausted was emitted before the grace even elapsed.
        assert_eq!(
            count_messages(&state, "ws-enf-kill", "budget_exhausted").await,
            1
        );
        // ... and the agent received it directly, so the grace period is
        // meaningful from the agent's side (hand-off signal).
        let inbox = agent_inbox(&state, "agent-enf-1").await;
        assert_eq!(
            inbox
                .iter()
                .filter(|m| m.kind.as_str() == "budget_exhausted")
                .count(),
            1,
            "agent inbox must contain budget_exhausted, got: {:?}",
            inbox.iter().map(|m| m.kind.as_str()).collect::<Vec<_>>()
        );

        handle.await.expect("grace timer must run to completion");

        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            agent.status,
            gyre_domain::AgentStatus::Dead,
            "still-active agent must be Dead after the grace period"
        );

        // Active-agent counter released.
        let usage = state
            .budget_usages
            .get_usage(&super::workspace_key("ws-enf-kill"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(usage.active_agents, 0, "kill must decrement active_agents");

        // Worktree preserved (record still present — not deleted).
        let wts = state
            .worktrees
            .find_by_agent(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap();
        assert_eq!(wts.len(), 1, "budget kill must preserve the worktree record");

        // Reassignment task created, referencing the killed agent's task.
        let tasks = state.tasks.list().await.unwrap();
        let reassignment = tasks
            .iter()
            .find(|t| {
                t.parent_task_id.as_ref() == Some(&gyre_common::Id::new("task-enf-1"))
            })
            .expect("reassignment task must be created for the killed agent's task");
        assert_eq!(
            reassignment.title,
            "Agent hit budget limit on task-enf-1, needs reassignment or budget increase"
        );
        assert_eq!(reassignment.workspace_id, gyre_common::Id::new("ws-enf-kill"));

        // Spawner was notified of the escalation.
        assert!(!budget_notifications(&state, "ws-enf-kill").await.is_empty());

        // Exhaustion de-dup: re-running enforcement in the same period must
        // not fire again (no second timer, no duplicate messages).
        assert!(
            super::check_and_enforce_budget_with_grace(&state, "ws-enf-kill", 0)
                .await
                .is_none(),
            "exhausted threshold must not re-fire within the same period"
        );
        assert_eq!(
            count_messages(&state, "ws-enf-kill", "budget_exhausted").await,
            1
        );
        assert_eq!(
            agent_inbox(&state, "agent-enf-1")
                .await
                .iter()
                .filter(|m| m.kind.as_str() == "budget_exhausted")
                .count(),
            1,
            "agent inbox must not receive a duplicate exhausted message in the same period"
        );
    }

    #[tokio::test]
    async fn exhausted_hardkill_override_kills_immediately_without_timer() {
        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-hard", 1000).await;
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-enf-hard"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    enforcement_action: Some(gyre_domain::BudgetEnforcementAction::HardKill),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        super::record_budget_usage(&state, "ws-enf-hard", 1000, 0.0).await;
        let handle = super::check_and_enforce_budget_with_grace(&state, "ws-enf-hard", 60).await;
        assert!(
            handle.is_none(),
            "HardKill must kill inline, not schedule a grace timer"
        );

        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            agent.status,
            gyre_domain::AgentStatus::Dead,
            "HardKill must terminate immediately"
        );
    }

    #[tokio::test]
    async fn exhausted_queuepause_override_pauses_workspace_merge_queues() {
        use gyre_domain::Repository;

        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-pause", 1000).await;
        state
            .repos
            .create(&Repository::new(
                gyre_common::Id::new("repo-enf-pause"),
                gyre_common::Id::new("ws-enf-pause"),
                "enf-repo",
                "/tmp/repos/enf-repo",
                0,
            ))
            .await
            .unwrap();
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-enf-pause"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    enforcement_action: Some(gyre_domain::BudgetEnforcementAction::QueuePause),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        super::record_budget_usage(&state, "ws-enf-pause", 1100, 0.0).await;
        let handle = super::check_and_enforce_budget_with_grace(&state, "ws-enf-pause", 0).await;
        assert!(handle.is_none(), "QueuePause must not schedule a kill timer");

        // The workspace repo's merge queue is paused via the shared
        // merge-processor pause state.
        let paused = state
            .kv_store
            .kv_get("merge_queue_pause", "repo-enf-pause")
            .await
            .unwrap()
            .expect("merge queue pause state must be persisted for the workspace repo");
        assert!(paused.contains("\"paused\":true"));

        // QueuePause must not kill the agent.
        let agent = state
            .agents
            .find_by_id(&gyre_common::Id::new("agent-enf-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.status, gyre_domain::AgentStatus::Active);
    }

    #[tokio::test]
    async fn exhausted_crossing_cost_limit_also_fires() {
        // The higher utilization across token AND cost limits drives
        // enforcement — a cost-only breach must fire too.
        let state = crate::mem::test_state();
        enforcement_fixture(&state, "ws-enf-cost", 1_000_000).await;
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-enf-cost"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1_000_000),
                    max_cost_per_day: Some(1.0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // 0.9/1.0 USD = 90%: warn tier.
        super::record_budget_usage(&state, "ws-enf-cost", 1, 0.9).await;
        super::check_and_enforce_budget(&state, "ws-enf-cost").await;
        assert_eq!(
            count_messages(&state, "ws-enf-cost", "budget_warning").await,
            1,
            "cost utilization >= 80% must fire the warning"
        );
    }

    #[tokio::test]
    async fn enforcement_action_persists_through_budget_api() {
        // PUT with an explicit enforcement_action, then read it back from
        // the same state's config store (the storage the GET and the
        // enforcement helper read).
        let state = crate::mem::test_state();
        let app = crate::build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-enf-api/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(
                        r#"{"max_tokens_per_day":1000,"enforcement_action":"hard_kill"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["config"]["enforcement_action"], "hard_kill");

        let config = state
            .budget_configs
            .get_config(&super::workspace_key("ws-enf-api"))
            .await
            .unwrap()
            .expect("config must persist through the budget API");
        assert_eq!(
            config.enforcement_action,
            Some(gyre_domain::BudgetEnforcementAction::HardKill)
        );
    }

    #[test]
    fn budget_enforcement_action_serde_round_trips() {
        for (action, name) in [
            (gyre_domain::BudgetEnforcementAction::Warn, "warn"),
            (gyre_domain::BudgetEnforcementAction::GracefulStop, "graceful_stop"),
            (gyre_domain::BudgetEnforcementAction::HardKill, "hard_kill"),
            (gyre_domain::BudgetEnforcementAction::QueuePause, "queue_pause"),
        ] {
            let s = serde_json::to_string(&action).unwrap();
            assert_eq!(s, format!("\"{name}\""));
            let back: gyre_domain::BudgetEnforcementAction =
                serde_json::from_str(&s).unwrap();
            assert_eq!(back, action);
        }
        // BudgetConfig round-trips the optional field in both states.
        let with_action = gyre_domain::BudgetConfig {
            enforcement_action: Some(gyre_domain::BudgetEnforcementAction::QueuePause),
            ..Default::default()
        };
        let round: gyre_domain::BudgetConfig =
            serde_json::from_str(&serde_json::to_string(&with_action).unwrap()).unwrap();
        assert_eq!(round.enforcement_action, with_action.enforcement_action);
        let default_round: gyre_domain::BudgetConfig =
            serde_json::from_str(&serde_json::to_string(&gyre_domain::BudgetConfig::default()).unwrap())
                .unwrap();
        assert_eq!(default_round.enforcement_action, None);
    }
}
