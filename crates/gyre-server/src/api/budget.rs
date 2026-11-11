/// Budget governance API — GET/PUT workspace budget limits, GET tenant summary.
///
/// Workspaces are keyed by project_id (the current governance boundary).
/// Budget state is persisted via port traits:
///   - `budget_configs` (BudgetRepository): entity_key -> BudgetConfig (limits)
///   - `budget_usages` (BudgetUsageRepository): entity_key -> BudgetUsage (real-time usage)
///
/// Entity keys: `"workspace:{project_id}"` or `"tenant:global"`.
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use gyre_common::Id;
use gyre_domain::{BudgetCallRecord, BudgetConfig, BudgetUsage};
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

/// Budget usage key for a repo-level budget (agent-runtime.md §4 Cascade:
/// per-agent usage is charged to the repo and rolled up to the workspace).
pub fn repo_key(repo_id: &str) -> String {
    format!("repo:{repo_id}")
}

fn new_ws_usage(entity_id: &str) -> BudgetUsage {
    BudgetUsage {
        entity_type: "workspace".into(),
        entity_id: gyre_common::Id::new(entity_id.to_string()),
        tokens_used_today: 0,
        cost_today: 0.0,
        active_agents: 0,
        period_start: now_secs(),
        compute_minutes: 0,
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
        compute_minutes: 0,
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
        if let (Some(ws), Some(t)) = (
            new_config.max_agent_lifetime_secs,
            tenant.max_agent_lifetime_secs,
        ) {
            if ws > t {
                return Err(ApiError::InvalidInput(format!(
                    "workspace max_agent_lifetime_secs ({ws}) exceeds tenant limit ({t})"
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

    // agent-runtime.md §4 Enforcement Levels — "Beyond limit": a budget
    // reduced below current usage while agents are running triggers an
    // immediate hard kill; a reduction to exactly 100% starts the graceful
    // stop. Enforcement runs synchronously so the PUT response reflects
    // the post-change enforcement state.
    // Fire-and-forget side-effecting enforcement (kills agents, emits
    // messages); the PUT result is the saved config, the kill outcome is
    // independently observable in agent status.
    enforce_workspace_budget(&state, &id, None, None, BUDGET_GRACE_PERIOD_SECS).await; // enforcement:ok — side-effecting kill/notify, no decision to consume

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
/// Uses the EFFECTIVE workspace limits (tighter of workspace config and the
/// tenant ceiling) so a lowered tenant ceiling binds immediately.
pub async fn check_spawn_budget(state: &AppState, project_id: &str) -> Result<(), String> {
    let key = workspace_key(project_id);
    let config = effective_workspace_config(state, &key).await;
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

    Ok(())
}

/// Check a repo-level budget override (agent-runtime.md §4 Cascade: "When no
/// repo-level budget is set, the repo inherits the workspace limit" — the
/// workspace check in `check_spawn_budget` covers that case). Called at
/// agent spawn time alongside the workspace check. Enforces every field of
/// the override: the daily token/cost ceilings and the concurrent-agent cap
/// (the lifetime ceiling is enforced continuously by
/// `enforce_agent_lifetimes`).
pub async fn check_repo_spawn_budget(state: &AppState, repo: &gyre_domain::Repository) -> Result<(), String> {
    let Some(budget) = repo.budget.as_ref() else {
        return Ok(()); // inherit the workspace limit
    };
    let key = repo_key(repo.id.as_str());
    let usage = state
        .budget_usages
        .get_usage(&key)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| BudgetUsage {
            entity_type: "repo".into(),
            entity_id: repo.id.clone(),
            tokens_used_today: 0,
            cost_today: 0.0,
            active_agents: 0,
            period_start: now_secs(),
            compute_minutes: 0,
        });
    if let Some(max_tokens) = budget.max_tokens_per_day {
        if usage.tokens_used_today >= max_tokens {
            return Err(format!(
                "repo budget exceeded: max_tokens_per_day={max_tokens} (used {} today)",
                usage.tokens_used_today
            ));
        }
    }
    if let Some(max_cost) = budget.max_cost_per_day {
        if usage.cost_today >= max_cost {
            return Err(format!(
                "repo budget exceeded: max_cost_per_day=${max_cost:.4} (spent ${:.4})",
                usage.cost_today
            ));
        }
    }
    // Repo-level concurrent-agent cap: a repo budget override of
    // max_concurrent_agents binds at spawn time exactly like the
    // workspace-level cap does (agent-runtime.md §4 Cascade — the repo
    // budget is the finer-grained control). Counted live from the agent
    // store: repo active-agent counts are not maintained incrementally
    // anywhere else, unlike the workspace/tenant counters.
    if let Some(max_concurrent) = budget.max_concurrent_agents {
        let active = count_active_agents_in_repo(state, repo).await;
        if active >= max_concurrent as usize {
            return Err(format!(
                "repo budget exceeded: max_concurrent_agents={max_concurrent} ({active} active in repo)"
            ));
        }
    }
    Ok(())
}

/// Count Active agents scoped to a repo. Two linkage paths, mirroring the
/// scheduler's `count_active_agents_for_repo` (signal_chain.rs): workers
/// link repos via their worktree records, orchestrators carry `repo_id`
/// directly. An agent reachable on either path is counted once.
async fn count_active_agents_in_repo(state: &AppState, repo: &gyre_domain::Repository) -> usize {
    let mut agent_ids: Vec<gyre_common::Id> = Vec::new();
    if let Ok(agents) = state.agents.list_by_workspace(&repo.workspace_id).await {
        for agent in agents {
            if agent.repo_id.as_ref() == Some(&repo.id) && !agent_ids.contains(&agent.id) {
                agent_ids.push(agent.id.clone());
            }
        }
    }
    if let Ok(worktrees) = state.worktrees.find_by_repo(&repo.id).await {
        for wt in worktrees {
            if !agent_ids.contains(&wt.agent_id) {
                agent_ids.push(wt.agent_id.clone());
            }
        }
    }
    let mut active = 0;
    for aid in &agent_ids {
        if let Ok(Some(a)) = state.agents.find_by_id(aid).await {
            if a.status == gyre_domain::AgentStatus::Active {
                active += 1;
            }
        }
    }
    active
}

/// Enforce per-agent lifetime ceilings (`max_agent_lifetime_secs`) across
/// every active agent: the EFFECTIVE limit is the repo budget override when
/// the agent is repo-bound and the repo sets one, else the effective
/// workspace limit (tighter of workspace config and tenant ceiling). An
/// agent whose wall-clock age (spawn → now) exceeds the limit is stopped
/// via the budget-kill path — `kill_process()`, worktree preserved, task
/// left InProgress — the same recovery semantics as every other budget
/// enforcement kill.
///
/// Runs after every usage report (agents actively burning budget are
/// checked continuously) and from the hourly budget job (agents that
/// stopped reporting are still bounded).
pub async fn enforce_agent_lifetimes(state: &AppState) {
    let agents = match state.agents.list().await {
        Ok(agents) => agents,
        Err(e) => {
            tracing::warn!("lifetime enforcement: agent list failed: {e}");
            return;
        }
    };
    let now = now_secs();
    for agent in agents
        .into_iter()
        .filter(|a| a.status == gyre_domain::AgentStatus::Active)
    {
        let Some(limit) = effective_lifetime_limit_secs(state, &agent).await else {
            continue; // unlimited
        };
        let age = now.saturating_sub(agent.spawned_at);
        if age > limit {
            tracing::warn!(
                agent_id = %agent.id,
                age_secs = age,
                limit_secs = limit,
                "agent lifetime exceeded: budget kill"
            );
            budget_stop_agent(state, &agent, "agent lifetime exceeded").await;
        }
    }
}

/// Effective `max_agent_lifetime_secs` for one agent: min(repo budget
/// override, effective workspace config). The repo override is
/// fine-grained control that can TIGHTEN the ceiling, never loosen it —
/// the cascade bound (repo ≤ workspace ≤ tenant) is recomputed live on
/// every enforcement pass, so a workspace or tenant budget reduced below
/// a previously-legitimate repo override binds immediately (the same
/// "beyond limit handles external budget changes" semantics the
/// token/cost/concurrent dimensions honor). `None` when neither the repo
/// nor the effective workspace config sets a limit.
async fn effective_lifetime_limit_secs(state: &AppState, agent: &gyre_domain::Agent) -> Option<u64> {
    // Repo binding via repo_id (orchestrators) or via a worktree
    // (workers carry no repo_id) — first repo with an override wins.
    let mut repo_ids: Vec<gyre_common::Id> = Vec::new();
    if let Some(repo_id) = agent.repo_id.as_ref() {
        repo_ids.push(repo_id.clone());
    }
    if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {
        for wt in worktrees {
            if !repo_ids.contains(&wt.repository_id) {
                repo_ids.push(wt.repository_id.clone());
            }
        }
    }
    let mut repo_limit: Option<u64> = None;
    for repo_id in &repo_ids {
        if let Ok(Some(repo)) = state.repos.find_by_id(repo_id).await {
            if let Some(limit) = repo.budget.as_ref().and_then(|b| b.max_agent_lifetime_secs) {
                repo_limit = Some(limit);
                break;
            }
        }
    }
    let ws_limit = effective_workspace_config(
        state,
        &workspace_key(agent.workspace_id.as_str()),
    )
    .await
    .max_agent_lifetime_secs;
    min_opt_u64(repo_limit, ws_limit)
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
/// trail and the aggregate counters can never diverge.
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
    let tokens = usage.input_tokens.saturating_add(usage.output_tokens);
    record_budget_usage(state, &usage.workspace_id.to_string(), tokens, usage.cost_usd).await;

    // agent-runtime.md §4 Cascade: agent usage is charged to the repo's
    // budget (fine-grained control) and rolls up to the workspace total.
    // User-initiated queries carry no repo context and charge the workspace
    // directly (§4 What's Tracked).
    if let Some(repo_id) = usage.repo_id.as_ref() {
        let now = now_secs();
        if let Err(e) = state
            .budget_usages
            .add_tokens_cost(
                &repo_key(repo_id.as_str()),
                "repo",
                repo_id.as_str(),
                now,
                tokens,
                usage.cost_usd,
            )
            .await
        {
            tracing::warn!(
                repo_id = %repo_id,
                "failed to charge repo budget usage: {e}"
            );
        }
    }

    // agent-runtime.md §4: "The server aggregates usage per workspace and
    // checks against workspace limits on every report."
    // Fire-and-forget side-effecting enforcement (kills agents, emits
    // messages, notifies); usage recording must not fail the caller's
    // request, and the kill outcome is independently observable in agent
    // status.
    enforce_workspace_budget( // enforcement:ok — side-effecting kill/notify, no decision to consume
        state,
        &usage.workspace_id.to_string(),
        usage.agent_id.as_ref(),
        usage.repo_id.as_ref(),
        BUDGET_GRACE_PERIOD_SECS,
    )
    .await;

    // agent-runtime.md §4 Cascade: the per-agent lifetime ceiling
    // (repo override → effective workspace limit) is checked on every
    // report too — an agent burning budget past its lifetime is stopped
    // without waiting for the hourly job.
    enforce_agent_lifetimes(state).await; // enforcement:ok — side-effecting kill, no decision to consume
}

// ── Threshold enforcement (agent-runtime.md §4 Enforcement Levels) ─────────────

/// Grace period after a workspace reaches 100% of a daily budget: active
/// agents receive `BudgetExhausted` and have this long to commit current
/// work and call `agent.complete`; agents still Active afterwards are
/// killed via `ComputeTarget::kill_process()` and marked `Stopped`.
pub const BUDGET_GRACE_PERIOD_SECS: u64 = 60;

/// Upper bound on per-call records scanned by the rolling-window reset
/// job. One cycle re-syncs the aggregate counters from the audit trail;
/// a workspace above this many LLM calls per 24h re-syncs on the next
/// hourly cycle (the incremental counters stay authoritative in between).
const ROLLING_WINDOW_SCAN_LIMIT: i64 = 100_000;

/// Highest configured daily-limit utilization fraction (tokens and cost).
/// `None` when no daily limit is configured — nothing to enforce.
fn utilization_pct(config: &BudgetConfig, usage: &BudgetUsage) -> Option<f64> {
    let token_pct = config
        .max_tokens_per_day
        .filter(|max| *max > 0)
        .map(|max| usage.tokens_used_today as f64 / max as f64);
    let cost_pct = config
        .max_cost_per_day
        .filter(|max| *max > 0.0)
        .map(|max| usage.cost_today / max);
    match (token_pct, cost_pct) {
        (Some(t), Some(c)) => Some(t.max(c)),
        (Some(t), None) => Some(t),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    }
}

fn min_opt_u64(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

fn min_opt_f64(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

fn min_opt_u32(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Effective workspace limits: each field is the tighter of the workspace
/// config and the tenant ceiling (agent-runtime.md §4 Cascade — the tenant
/// budget is the absolute ceiling even when the workspace config is
/// looser, e.g. after a tenant admin lowered the ceiling).
pub(crate) async fn effective_workspace_config(state: &AppState, ws_key: &str) -> BudgetConfig {
    let ws = state
        .budget_configs
        .get_config(ws_key)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let tenant = state
        .budget_configs
        .get_config(tenant_key())
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    BudgetConfig {
        max_tokens_per_day: min_opt_u64(ws.max_tokens_per_day, tenant.max_tokens_per_day),
        max_cost_per_day: min_opt_f64(ws.max_cost_per_day, tenant.max_cost_per_day),
        max_concurrent_agents: min_opt_u32(
            ws.max_concurrent_agents,
            tenant.max_concurrent_agents,
        ),
        max_agent_lifetime_secs: min_opt_u64(
            ws.max_agent_lifetime_secs,
            tenant.max_agent_lifetime_secs,
        ),
    }
}

/// Check a workspace's aggregated usage against its effective limits and
/// enforce agent-runtime.md §4 Enforcement Levels:
///
/// - ≥ 80%: `BudgetWarning` notification (Inbox) + `BudgetWarning` event on
///   the message bus. Agents continue running.
/// - ≥ 100%: a `BudgetExhausted` message delivered to every active agent's
///   inbox (`Destination::Agent` — readable via `list_unacked`/
///   `list_after`, the surfaces agents actually poll), a `grace_secs` grace
///   period to commit + complete, then `kill_process()` on agents still
///   Active (marked `Stopped`; worktree preserved; task remains InProgress).
/// - Beyond limit (usage strictly greater than the limit — the budget was
///   reduced below current usage): immediate `kill_process()`, no grace.
///
/// Called after every usage report and after every workspace budget
/// update. Best-effort: enforcement failures are logged, never propagated
/// to the caller (usage recording must not fail the request).
pub async fn enforce_workspace_budget(
    state: &Arc<AppState>,
    workspace_id: &str,
    trigger_agent: Option<&Id>,
    trigger_repo: Option<&Id>,
    grace_secs: u64,
) {
    let ws_key = workspace_key(workspace_id);
    let effective = effective_workspace_config(state, &ws_key).await;
    let usage = match state.budget_usages.get_usage(&ws_key).await {
        Ok(Some(u)) => u,
        Ok(None) => new_ws_usage(workspace_id),
        Err(e) => {
            tracing::warn!(workspace_id, "budget usage read failed: {e}");
            return;
        }
    };

    let Some(pct) = utilization_pct(&effective, &usage) else {
        return;
    };

    if pct > 1.0 {
        // Beyond limit — hard kill, immediately (agent-runtime.md §4).
        tracing::warn!(
            workspace_id,
            usage_pct = format!("{:.2}", pct),
            "workspace budget beyond limit: immediate hard kill of active agents"
        );
        let workspace = Id::new(workspace_id.to_string());
        let agents = active_workspace_agents(state, &workspace).await;
        for agent in &agents {
            state
                .emit_event(
                    Some(workspace.clone()),
                    gyre_common::Destination::Agent(agent.id.clone()),
                    gyre_common::MessageKind::BudgetExhausted,
                    Some(serde_json::json!({
                        "agent_id": agent.id.as_str(),
                        "workspace_id": workspace_id,
                        "grace_secs": 0u64,
                    })),
                )
                .await;
        }
        notify_budget_level(
            state,
            &workspace,
            &effective,
            &usage,
            pct,
            true,
            trigger_agent,
            trigger_repo,
        )
        .await;
        kill_workspace_agents_for_budget(state, &workspace, "budget beyond limit").await;
        return;
    }

    if pct >= 1.0 {
        // 100% — graceful stop: notify every active agent, then grace timer.
        tracing::warn!(
            workspace_id,
            usage_pct = format!("{:.2}", pct),
            grace_secs,
            "workspace budget exhausted: graceful stop with grace period"
        );
        let workspace = Id::new(workspace_id.to_string());
        let agents = active_workspace_agents(state, &workspace).await;
        for agent in &agents {
            state
                .emit_event(
                    Some(workspace.clone()),
                    gyre_common::Destination::Agent(agent.id.clone()),
                    gyre_common::MessageKind::BudgetExhausted,
                    Some(serde_json::json!({
                        "agent_id": agent.id.as_str(),
                        "workspace_id": workspace_id,
                        "grace_secs": grace_secs,
                    })),
                )
                .await;
        }
        notify_budget_level(
            state,
            &workspace,
            &effective,
            &usage,
            pct,
            true,
            trigger_agent,
            trigger_repo,
        )
        .await;
        start_budget_grace_timer(Arc::clone(state), workspace, grace_secs).await;
        return;
    }

    if pct >= 0.8 {
        // 80% — warn; agents continue.
        let workspace = Id::new(workspace_id.to_string());
        let created = notify_budget_level(
            state,
            &workspace,
            &effective,
            &usage,
            pct,
            false,
            trigger_agent,
            trigger_repo,
        )
        .await;
        // The BudgetWarning bus event rides the same level-crossing gate
        // as the inbox notification (created > 0) so repeated usage reports
        // within one period do not spam the event stream.
        if created > 0 {
            if let Some(agent_id) = trigger_agent {
                state
                    .emit_event(
                        Some(workspace.clone()),
                        gyre_common::Destination::Workspace(workspace.clone()),
                        gyre_common::MessageKind::BudgetWarning,
                        Some(serde_json::json!({
                            "agent_id": agent_id.as_str(),
                            "workspace_id": workspace_id,
                            "usage_pct": (pct * 100.0).round(),
                        })),
                    )
                    .await;
            }
        }
    }
}

/// Active (running) agents in a workspace — the enforcement targets.
async fn active_workspace_agents(state: &AppState, workspace_id: &Id) -> Vec<gyre_domain::Agent> {
    state
        .agents
        .list_by_workspace(workspace_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.status == gyre_domain::AgentStatus::Active)
        .collect()
}

/// Start the 60s grace timer for a workspace (at most one live timer per
/// workspace — repeated usage reports at ≥100% must not stack timers).
/// When the timer fires, agents still Active in the workspace are killed
/// via `ComputeTarget::kill_process()` and marked `Stopped` — unless the
/// budget was raised or the rolling window expired entries during the
/// grace period, in which case enforcement stands down.
async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {
    {
        let mut guard = state.budget_grace_in_progress.lock().await;
        if guard.contains(workspace_id.as_str()) {
            return;
        }
        guard.insert(workspace_id.to_string());
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(grace_secs)).await;
        state
            .budget_grace_in_progress
            .lock()
            .await
            .remove(workspace_id.as_str());
        // Re-evaluate before killing: the limits or usage may have changed
        // during the grace period (budget raised, entries rolled off).
        let ws_key = workspace_key(workspace_id.as_str());
        let effective = effective_workspace_config(&state, &ws_key).await;
        let still_exhausted = match state.budget_usages.get_usage(&ws_key).await {
            Ok(Some(usage)) => utilization_pct(&effective, &usage)
                .map(|pct| pct >= 1.0)
                .unwrap_or(false),
            Ok(None) => false,
            Err(_) => true, // cannot determine state — enforce, never fail open
        };
        if still_exhausted {
            kill_workspace_agents_for_budget(
                &state,
                &workspace_id,
                "budget grace period expired",
            )
            .await;
        }
    });
}

/// Kill every active agent in the workspace for budget enforcement
/// (agent-runtime.md §4). Worktrees are preserved and tasks remain
/// InProgress for human review — recovery, not failure.
async fn kill_workspace_agents_for_budget(
    state: &AppState,
    workspace_id: &Id,
    reason: &str,
) -> Vec<Id> {
    let agents = active_workspace_agents(state, workspace_id).await;
    let mut killed = Vec::new();
    for agent in &agents {
        budget_stop_agent(state, agent, reason).await;
        killed.push(agent.id.clone());
    }
    killed
}

/// Stop one agent for budget enforcement: mark `Stopped`, kill its process
/// via the compute backend recorded at spawn, revoke its token, charge its
/// wall-clock compute minutes, and release its budget slot.
///
/// Deliberately unlike the admin kill route: the worktree is preserved
/// (recovery) and the task is NOT blocked — it remains `InProgress` for
/// human review (agent-runtime.md §4 "Beyond limit").
pub async fn budget_stop_agent(state: &AppState, agent: &gyre_domain::Agent, reason: &str) {
    if agent.status != gyre_domain::AgentStatus::Active {
        return; // completed within the grace period (Idle) or already gone
    }
    let mut stopped = agent.clone();
    if let Err(e) = stopped.transition_status(gyre_domain::AgentStatus::Stopped) {
        tracing::warn!(agent_id = %agent.id, "budget stop transition failed: {e}");
        return;
    }
    if let Err(e) = state.agents.update(&stopped).await {
        tracing::warn!(agent_id = %agent.id, "budget stop persist failed: {e}");
    }

    // Kill the actual process/container/Pod. The backend recorded at spawn
    // time decides how (docker rm --force / kubectl delete pod / remote
    // docker rm); LocalTarget (pid kill) only for agents whose backend is
    // unknown (pre-task-117 records).
    let mut killed_pid: Option<u32> = None;
    if let Some(handle) = state.process_registry.lock().await.remove(agent.id.as_str()) {
        killed_pid = handle.pid;
        let backend = state.spawned_backends.lock().await.remove(agent.id.as_str());
        let kill_result = match backend.as_deref() {
            Some(b) => gyre_ports::ComputeTarget::kill_process(b, &handle).await,
            None => {
                gyre_ports::ComputeTarget::kill_process(
                    &gyre_adapters::compute::LocalTarget,
                    &handle,
                )
                .await
            }
        };
        if let Err(e) = kill_result {
            tracing::warn!(agent_id = %agent.id, "budget kill_process failed: {e}");
        }
    }
    if killed_pid.is_none() {
        killed_pid = crate::agent_audit::attested_pid(state, agent.id.as_str()).await;
    }

    // The agent process is dead — its token and audit trail must not
    // outlive it.
    crate::agent_audit::revoke_agent_token(state, &stopped, reason).await;
    crate::agent_audit::emit_killed(
        state,
        &stopped,
        reason,
        "budget-enforcement",
        killed_pid,
    )
    .await;

    // Charge wall-clock compute minutes and release the budget slot.
    record_compute_minutes_for_agent(state, &stopped).await;
    decrement_active_agents(state, &stopped.workspace_id.to_string()).await;
}

/// Charge an agent's wall-clock compute time (spawn → complete/kill), in
/// whole minutes, to the workspace and tenant usage entries — and to the
/// repo entry when the agent is repo-bound (agent-runtime.md §4 What's
/// Tracked: `compute_minutes`).
pub async fn record_compute_minutes_for_agent(state: &AppState, agent: &gyre_domain::Agent) {
    let now = now_secs();
    let minutes = now.saturating_sub(agent.spawned_at) / 60;
    if minutes == 0 {
        return; // whole-minute accounting: sub-minute runs contribute 0
    }
    let ws_id = agent.workspace_id.to_string();
    if let Err(e) = state
        .budget_usages
        .add_compute_minutes(&workspace_key(&ws_id), "workspace", &ws_id, now, minutes)
        .await
    {
        tracing::warn!(agent_id = %agent.id, "compute-minute accounting failed: {e}");
    }
    if let Err(e) = state
        .budget_usages
        .add_compute_minutes(tenant_key(), "tenant", "global", now, minutes)
        .await
    {
        tracing::warn!(agent_id = %agent.id, "tenant compute-minute accounting failed: {e}");
    }
    if let Some(repo_id) = agent.repo_id.as_ref() {
        let _ = state
            .budget_usages
            .add_compute_minutes(&repo_key(repo_id.as_str()), "repo", repo_id.as_str(), now, minutes)
            .await;
    }
}

/// user-management.md §Notification Entity: `AgentBudgetWarning` (≥ 80% of a
/// daily limit, High — needs attention soon) and `AgentBudgetExhausted`
/// (≥ 100%, Urgent — work is stopped until the period resets).
///
/// Recipients follow the spec's "Who Gets Notified" table: the agent's
/// spawning user plus workspace Admins (warning) / the workspace Owner
/// (exhausted). Emitted at most once per user per budget period via
/// `has_active_notification` so repeated usage reports within the same
/// period do not spam the inbox. Returns the number of notifications
/// newly created (0 when every recipient was already notified — the
/// level-crossing signal the BudgetWarning bus event rides on).
async fn notify_budget_level(
    state: &AppState,
    workspace_id: &Id,
    config: &BudgetConfig,
    ws_usage: &BudgetUsage,
    pct: f64,
    exhausted: bool,
    agent_id: Option<&Id>,
    repo_id: Option<&Id>,
) -> usize {
    // The tenant scope comes from the workspace lookup — when it cannot be
    // resolved there is no scoperable tenant, so skip rather than fabricate
    // one (check-fabricated-scope-defaults.sh).
    let Ok(Some(ws)) = state.workspaces.find_by_id(workspace_id).await else {
        return 0;
    };
    let tenant_id = ws.tenant_id.to_string();

    let (ntype, title) = if exhausted {
        (
            gyre_common::NotificationType::AgentBudgetExhausted,
            format!(
                "Workspace budget exhausted — {:.0}% of the daily limit used",
                (pct * 100.0).min(999.0)
            ),
        )
    } else {
        (
            gyre_common::NotificationType::AgentBudgetWarning,
            format!("Workspace budget at {:.0}% of the daily limit", pct * 100.0),
        )
    };

    // Recipients: the agent's spawning user (when the usage is
    // agent-attributed) plus workspace Admins (warning) / Owner (exhausted).
    let mut recipients: Vec<gyre_common::Id> = Vec::new();
    if let Some(agent_id) = agent_id {
        if let Ok(Some(agent)) = state.agents.find_by_id(agent_id).await {
            if let Some(spawned_by) = agent.spawned_by {
                recipients.push(gyre_common::Id::new(spawned_by));
            }
        }
    }
    if let Ok(members) = state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
    {
        recipients.extend(
            members
                .into_iter()
                .filter(|m| {
                    if exhausted {
                        matches!(m.role, gyre_domain::WorkspaceRole::Owner)
                    } else {
                        matches!(m.role, gyre_domain::WorkspaceRole::Admin)
                    }
                })
                .map(|m| m.user_id),
        );
    }
    recipients.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    recipients.dedup_by(|a, b| a.as_str() == b.as_str());

    // De-duplicate per period: the budget counters increment on every call,
    // so without this guard each call past the threshold would re-notify.
    let entity_ref = workspace_id.to_string();
    let since = ws_usage.period_start as i64;
    let mut created = 0usize;
    for user_id in recipients {
        let already_notified = state
            .notifications
            .has_active_notification(
                workspace_id,
                &user_id,
                ntype.as_str(),
                &entity_ref,
                since,
            )
            .await
            .unwrap_or(false);
        if already_notified {
            continue;
        }
        let body = serde_json::json!({
            "workspace_id": workspace_id.as_str(),
            "usage_pct": (pct * 100.0).round(),
            "tokens_used_today": ws_usage.tokens_used_today,
            "max_tokens_per_day": config.max_tokens_per_day,
            "cost_today": ws_usage.cost_today,
            "max_cost_per_day": config.max_cost_per_day,
            "agent_id": agent_id.map(|a| a.as_str()),
        })
        .to_string();
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            user_id,
            ntype.clone(),
            title.clone(),
            &tenant_id,
            Some(body),
            Some(entity_ref.clone()),
            repo_id.map(|r| r.to_string()),
            "budget",
            None,
        )
        .await;
        created += 1;
    }
    created
}

/// Apply the rolling 24-hour budget window (agent-runtime.md §4 Budget
/// Reset): recompute every entity's `tokens_used_today` / `cost_today`
/// counters from the per-call audit records inside the window
/// (`timestamp >= now - 24h`), so usage older than the window stops
/// counting — time-based budgets (`max_tokens_per_day`,
/// `max_cost_per_day`) reset on a rolling window, not a calendar day.
///
/// Runs hourly (`spawn_budget_reset` job). `compute_minutes` is cumulative
/// tracking (no time-based limit consumes it) and `active_agents` is a
/// live gauge — neither is windowed. A record saved between the scan and
/// the counter write is picked up by the next hourly cycle; the
/// incremental counters stay authoritative in between.
pub async fn apply_rolling_24h_window(state: &AppState) -> anyhow::Result<()> {
    let now = now_secs();
    let cutoff = now.saturating_sub(24 * 60 * 60);
    let records = state
        .budget_calls
        .list_since(cutoff, ROLLING_WINDOW_SCAN_LIMIT)
        .await?;

    let mut ws_sums: std::collections::HashMap<String, (u64, f64)> =
        std::collections::HashMap::new();
    let mut repo_sums: std::collections::HashMap<String, (u64, f64)> =
        std::collections::HashMap::new();
    let mut tenant_total = (0u64, 0.0f64);
    for r in &records {
        let tokens = r.input_tokens.saturating_add(r.output_tokens);
        let e = ws_sums.entry(r.workspace_id.to_string()).or_default();
        e.0 = e.0.saturating_add(tokens);
        e.1 += r.cost_usd;
        if let Some(repo_id) = r.repo_id.as_ref() {
            let e = repo_sums.entry(repo_id.to_string()).or_default();
            e.0 = e.0.saturating_add(tokens);
            e.1 += r.cost_usd;
        }
        tenant_total.0 = tenant_total.0.saturating_add(tokens);
        tenant_total.1 += r.cost_usd;
    }

    let entries = state.budget_usages.list_all_usage().await?;
    let mut updated = 0usize;
    for (key, mut entry) in entries {
        let (tokens, cost) = match entry.entity_type.as_str() {
            "workspace" => ws_sums
                .get(entry.entity_id.as_str())
                .copied()
                .unwrap_or((0, 0.0)),
            "repo" => repo_sums
                .get(entry.entity_id.as_str())
                .copied()
                .unwrap_or((0, 0.0)),
            // The tenant entry is the storage-wide roll-up (single
            // `tenant:global` ceiling per deployment).
            "tenant" => tenant_total,
            _ => continue,
        };
        if entry.tokens_used_today == tokens
            && (entry.cost_today - cost).abs() < f64::EPSILON
            && entry.period_start == cutoff
        {
            continue;
        }
        entry.tokens_used_today = tokens;
        entry.cost_today = cost;
        entry.period_start = cutoff;
        state.budget_usages.set_usage(&key, &entry).await?;
        updated += 1;
    }
    tracing::info!(
        cutoff,
        records_scanned = records.len(),
        entries_updated = updated,
        "budget rolling 24h window applied"
    );
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
                "tenant_id": "default",
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
                "tenant_id": "default",
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

    /// user-management.md §Notification Entity: AgentBudgetWarning (≥ 80%,
    /// High) and AgentBudgetExhausted (≥ 100%, Urgent) fire for the agent's
    /// spawning user plus workspace Admins (warning) / the workspace Owner
    /// (exhausted), at most once per budget period, and never below 80%.
    #[tokio::test]
    async fn budget_threshold_notifications_fire_once_per_period() {
        use gyre_domain::{Agent, Workspace, WorkspaceMembership, WorkspaceRole};

        let state = crate::mem::test_state();
        let ws_id = gyre_common::Id::new("ws-bn");
        let ws = Workspace::new(
            ws_id.clone(),
            gyre_common::Id::new("tenant-1"),
            "budget-ws",
            "budget-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let members = [
            ("user-admin", WorkspaceRole::Admin),
            ("user-owner", WorkspaceRole::Owner),
            ("user-dev", WorkspaceRole::Developer),
        ];
        for (uid, role) in members {
            let m = WorkspaceMembership::new(
                crate::api::new_id(),
                gyre_common::Id::new(uid),
                ws_id.clone(),
                role,
                gyre_common::Id::new("inviter"),
                0,
            );
            state.workspace_memberships.create(&m).await.unwrap();
        }

        let mut agent = Agent::new(gyre_common::Id::new("agent-bn"), "spender", 0);
        agent.spawned_by = Some("user-spawner".to_string());
        agent.workspace_id = ws_id.clone();
        state.agents.create(&agent).await.unwrap();

        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-bn"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let usage = |tokens: u64| super::LlmCallUsage {
            tenant_id: gyre_common::Id::new("tenant-1"),
            workspace_id: ws_id.clone(),
            repo_id: None,
            agent_id: Some(gyre_common::Id::new("agent-bn")),
            task_id: None,
            usage_type: "agent_run".to_string(),
            input_tokens: tokens,
            output_tokens: 0,
            cost_usd: 0.0,
            model: "test-model".to_string(),
        };

        async fn all(state: &crate::AppState) -> Vec<gyre_common::Notification> {
            gyre_ports::NotificationRepository::list_recent(state.notifications.as_ref(), 100)
                .await
                .unwrap()
        }

        // 50%: below the warning threshold — no notification at all.
        super::record_llm_call_usage(&state, &usage(500)).await;
        assert!(
            all(state.as_ref()).await.is_empty(),
            "no budget notification below 80%"
        );

        // 85%: warning to the spawning user + workspace Admins only.
        super::record_llm_call_usage(&state, &usage(350)).await;
        let warnings: Vec<_> = all(&state)
            .await
            .into_iter()
            .filter(|n| n.notification_type == gyre_common::NotificationType::AgentBudgetWarning)
            .collect();
        let mut warned: Vec<&str> = warnings.iter().map(|n| n.user_id.as_str()).collect();
        warned.sort_unstable();
        assert_eq!(
            warned,
            vec!["user-admin", "user-spawner"],
            "warning routes to spawner + Admins, not Owner/Developer"
        );
        for n in &warnings {
            assert_eq!(
                n.priority,
                gyre_common::NotificationPriority::High,
                "budget warning is High per user-management.md"
            );
            assert_eq!(n.source.entity_type, "budget");
        }

        // Another call in the same period (now ≥ 90%): still no duplicate.
        super::record_llm_call_usage(&state, &usage(100)).await;
        let warning_count = all(&state)
            .await
            .iter()
            .filter(|n| {
                n.notification_type == gyre_common::NotificationType::AgentBudgetWarning
                    && n.user_id.as_str() == "user-spawner"
            })
            .count();
        assert_eq!(warning_count, 1, "warning must not re-fire within a period");

        // Past 100%: exhausted (Urgent) to the spawning user + workspace Owner.
        super::record_llm_call_usage(&state, &usage(200)).await;
        let exhausted: Vec<_> = all(&state)
            .await
            .into_iter()
            .filter(|n| n.notification_type == gyre_common::NotificationType::AgentBudgetExhausted)
            .collect();
        let mut exhausted_users: Vec<&str> = exhausted.iter().map(|n| n.user_id.as_str()).collect();
        exhausted_users.sort_unstable();
        assert_eq!(
            exhausted_users,
            vec!["user-owner", "user-spawner"],
            "exhaustion routes to spawner + Owner"
        );
        for n in &exhausted {
            assert_eq!(
                n.priority,
                gyre_common::NotificationPriority::Urgent,
                "budget exhausted is Urgent per user-management.md"
            );
        }
    }

    // ── task-119: agent-runtime.md §4 enforcement + §5 prompt structure ──────

    use gyre_domain::{Agent, AgentStatus, Task, TaskStatus, Workspace};

    /// Shared fixture: a workspace, a config with a 1000-token daily limit,
    /// and an Active agent bound to an InProgress task with a worktree.
    fn now_secs_static() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    async fn enforcement_fixture(ws_name: &str) -> (Arc<crate::AppState>, gyre_common::Id, gyre_common::Id, gyre_common::Id) {
        let state = crate::mem::test_state();
        let ws_id = gyre_common::Id::new(ws_name);
        let ws = Workspace::new(
            ws_id.clone(),
            gyre_common::Id::new("tenant-1"),
            ws_name,
            ws_name,
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        state
            .budget_configs
            .set_config(
                &super::workspace_key(ws_name),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // InProgress task + Active agent.
        let task_id = gyre_common::Id::new(format!("{ws_name}-task"));
        let mut task = Task::new(task_id.clone(), "budget enforcement task", 0);
        task.workspace_id = ws_id.clone();
        task.repo_id = gyre_common::Id::new(format!("{ws_name}-repo"));
        task.status = TaskStatus::InProgress;
        state.tasks.create(&task).await.unwrap();

        let agent_id = gyre_common::Id::new(format!("{ws_name}-agent"));
        let mut agent = Agent::new(agent_id.clone(), "spender", now_secs_static());
        agent.workspace_id = ws_id.clone();
        agent.status = AgentStatus::Active;
        agent.current_task_id = Some(task_id.clone());
        // A spawning user gives the 80% warning a notification recipient —
        // the BudgetWarning bus event rides the same level-crossing gate.
        agent.spawned_by = Some("user-spawner".to_string());
        state.agents.create(&agent).await.unwrap();

        // A worktree that budget kills must PRESERVE.
        let wt = gyre_domain::AgentWorktree::new(
            gyre_common::Id::new(format!("{ws_name}-wt")),
            agent_id.clone(),
            gyre_common::Id::new(format!("{ws_name}-repo")),
            Some(task_id.clone()),
            format!("feat/{ws_name}"),
            format!("/tmp/wt-{ws_name}"),
            0,
        );
        state.worktrees.create(&wt).await.unwrap();

        (state, ws_id, agent_id, task_id)
    }

    async fn workspace_messages(
        state: &crate::AppState,
        ws_id: &gyre_common::Id,
    ) -> Vec<gyre_common::Message> {
        state
            .messages
            .list_by_workspace(ws_id, None, None, None, None, Some(50))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn usage_report_at_80pct_emits_budget_warning_message() {
        let (state, ws_id, agent_id, _) =
            enforcement_fixture("ws-warn-msg").await;
        // 850/1000 = 85% — warning band.
        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-1"),
                workspace_id: ws_id.clone(),
                repo_id: None,
                agent_id: Some(agent_id.clone()),
                task_id: None,
                usage_type: "agent_run".to_string(),
                input_tokens: 850,
                output_tokens: 0,
                cost_usd: 0.0,
                model: "test-model".to_string(),
            },
        )
        .await;

        let msgs = workspace_messages(state.as_ref(), &ws_id).await;
        let warnings: Vec<_> = msgs
            .iter()
            .filter(|m| m.kind == gyre_common::MessageKind::BudgetWarning)
            .collect();
        assert_eq!(warnings.len(), 1, "one BudgetWarning bus event at 80%+");
        assert_eq!(
            warnings[0].payload.as_ref().unwrap()["agent_id"],
            agent_id.as_str()
        );
        assert_eq!(
            warnings[0].payload.as_ref().unwrap()["workspace_id"],
            ws_id.as_str()
        );
        // Agents continue: the agent must still be Active after a warning.
        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(agent.status, AgentStatus::Active);
    }

    #[tokio::test]
    async fn exhaustion_emits_messages_and_kills_after_grace() {
        let (state, _ws_id, agent_id, task_id) =
            enforcement_fixture("ws-grace").await;
        // Push usage to exactly 100%.
        state
            .budget_usages
            .add_tokens_cost(
                &super::workspace_key("ws-grace"),
                "workspace",
                "ws-grace",
                0,
                1000,
                0.0,
            )
            .await
            .unwrap();

        // grace 0: the grace timer fires immediately.
        super::enforce_workspace_budget(&state, "ws-grace", None, None, 0).await;

        // BudgetExhausted delivered to the active agent's INBOX — the
        // surface agents actually read (list_unacked/list_after return
        // only Destination::Agent messages), per agent-runtime.md §4:
        // "All active agents in the workspace receive a BudgetExhausted
        // message in their inbox."
        let inbox = state
            .messages
            .list_unacked(&agent_id, 50)
            .await
            .unwrap();
        let exhausted: Vec<_> = inbox
            .iter()
            .filter(|m| m.kind == gyre_common::MessageKind::BudgetExhausted)
            .collect();
        assert_eq!(exhausted.len(), 1, "active agent must receive BudgetExhausted in its inbox");
        assert_eq!(exhausted[0].to, gyre_common::Destination::Agent(agent_id.clone()));
        assert_eq!(
            exhausted[0].payload.as_ref().unwrap()["grace_secs"],
            serde_json::json!(0u64)
        );

        // The grace timer runs on the tokio runtime — poll for the kill.
        let mut killed = false;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
            if agent.status == AgentStatus::Stopped {
                killed = true;
                break;
            }
        }
        assert!(killed, "agent must be killed (Stopped) after the grace period");

        // Worktree preserved + task remains InProgress for human review.
        let wts = state.worktrees.find_by_agent(&agent_id).await.unwrap();
        assert_eq!(wts.len(), 1, "budget kill must preserve the worktree");
        let task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(
            task.status,
            TaskStatus::InProgress,
            "task must remain InProgress after a budget kill"
        );
    }

    #[tokio::test]
    async fn beyond_limit_kills_immediately_without_grace() {
        let (state, ws_id, agent_id, task_id) =
            enforcement_fixture("ws-beyond").await;
        // Usage 1500 vs limit 1000 — the budget was "reduced" below usage.
        state
            .budget_usages
            .add_tokens_cost(
                &super::workspace_key("ws-beyond"),
                "workspace",
                "ws-beyond",
                0,
                1500,
                0.0,
            )
            .await
            .unwrap();

        super::enforce_workspace_budget(&state, "ws-beyond", None, None, 60).await;

        // Immediate, synchronous kill — no grace period.
        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "beyond-limit must hard-kill immediately"
        );
        let msgs = workspace_messages(state.as_ref(), &ws_id).await;
        assert!(
            !msgs.iter().any(|m| m.kind == gyre_common::MessageKind::BudgetExhausted),
            "beyond-limit BudgetExhausted goes to agent inboxes, not the workspace stream"
        );
        // ...and it IS in the agent's inbox (the surface agents read).
        let inbox = state
            .messages
            .list_unacked(&agent_id, 50)
            .await
            .unwrap();
        assert!(
            inbox.iter().any(|m| m.kind == gyre_common::MessageKind::BudgetExhausted),
            "beyond-limit must deliver BudgetExhausted to the agent inbox"
        );
        let wts = state.worktrees.find_by_agent(&agent_id).await.unwrap();
        assert_eq!(wts.len(), 1, "worktree preserved for recovery");
        let task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn budget_reduction_below_usage_triggers_beyond_limit_kill() {
        // The PUT path: an admin lowers the workspace budget below current
        // usage while an agent is running → immediate kill.
        let (state, ws_id, agent_id, _) = enforcement_fixture("ws-reduce").await;
        state
            .budget_usages
            .add_tokens_cost(
                &super::workspace_key("ws-reduce"),
                "workspace",
                "ws-reduce",
                0,
                900,
                0.0,
            )
            .await
            .unwrap();

        let app = crate::build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-reduce/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_tokens_per_day":500}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "reducing the budget below usage must hard-kill running agents"
        );
        assert_eq!(agent.workspace_id, ws_id);
    }

    #[tokio::test]
    async fn compute_minutes_tracked_on_terminal_transition() {
        let state = crate::mem::test_state();
        let ws_id = gyre_common::Id::new("ws-minutes");
        let ws = Workspace::new(
            ws_id.clone(),
            gyre_common::Id::new("tenant-1"),
            "minutes",
            "minutes",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Spawned 150 seconds ago → 2 whole minutes.
        let mut agent = Agent::new(gyre_common::Id::new("agent-min"), "runner", 0);
        agent.workspace_id = ws_id.clone();
        agent.spawned_at = now_secs_static().saturating_sub(150);
        state.agents.create(&agent).await.unwrap();

        super::record_compute_minutes_for_agent(state.as_ref(), &agent).await;
        let usage = state
            .budget_usages
            .get_usage(&super::workspace_key("ws-minutes"))
            .await
            .unwrap()
            .expect("usage entry must exist");
        assert_eq!(usage.compute_minutes, 2, "150s of wall clock = 2 minutes");

        let tenant_usage = state
            .budget_usages
            .get_usage(super::tenant_key())
            .await
            .unwrap()
            .expect("tenant usage entry must exist");
        assert_eq!(tenant_usage.compute_minutes, 2);
    }

    #[tokio::test]
    async fn rolling_24h_window_clears_expired_usage() {
        let state = crate::mem::test_state();
        let now = now_secs_static();
        let ws_id = gyre_common::Id::new("ws-roll");
        let mk_record = |id: &str, ts: u64, tokens: u64| gyre_domain::BudgetCallRecord {
            id: gyre_common::Id::new(id),
            tenant_id: gyre_common::Id::new("tenant-1"),
            workspace_id: ws_id.clone(),
            repo_id: None,
            agent_id: None,
            task_id: None,
            usage_type: "agent_run".to_string(),
            input_tokens: tokens,
            output_tokens: 0,
            cost_usd: 0.0,
            model: "m".to_string(),
            timestamp: ts,
        };
        // One record 25h old (expired), one 2h old (in window).
        state
            .budget_calls
            .save(&mk_record("old", now - 25 * 3600, 700))
            .await
            .unwrap();
        state
            .budget_calls
            .save(&mk_record("new", now - 2 * 3600, 300))
            .await
            .unwrap();

        // Counters currently hold both (as if incremented when reported).
        state
            .budget_usages
            .add_tokens_cost(
                &super::workspace_key("ws-roll"),
                "workspace",
                "ws-roll",
                now - 25 * 3600,
                1000,
                0.0,
            )
            .await
            .unwrap();

        super::apply_rolling_24h_window(state.as_ref()).await.unwrap();

        let usage = state
            .budget_usages
            .get_usage(&super::workspace_key("ws-roll"))
            .await
            .unwrap()
            .expect("usage entry must survive the roll");
        assert_eq!(
            usage.tokens_used_today, 300,
            "only the in-window record may count after the rolling reset"
        );
        // The job computes its own `now` — allow a small skew across a
        // second boundary, but it must be the rolling cutoff, not a
        // calendar-day boundary.
        let expected = now - 24 * 3600;
        assert!(
            (usage.period_start as i64 - expected as i64).abs() <= 2,
            "period_start must be the rolling window cutoff (±2s), got {} vs {expected}",
            usage.period_start
        );
    }

    #[tokio::test]
    async fn repo_usage_rolls_up_from_agent_reports() {
        let state = crate::mem::test_state();
        let repo_id = gyre_common::Id::new("repo-rollup");
        super::record_llm_call_usage(
            &state,
            &super::LlmCallUsage {
                tenant_id: gyre_common::Id::new("tenant-1"),
                workspace_id: gyre_common::Id::new("ws-rollup"),
                repo_id: Some(repo_id.clone()),
                agent_id: None,
                task_id: None,
                usage_type: "agent_run".to_string(),
                input_tokens: 600,
                output_tokens: 400,
                cost_usd: 0.10,
                model: "m".to_string(),
            },
        )
        .await;

        let repo_usage = state
            .budget_usages
            .get_usage(&super::repo_key(repo_id.as_str()))
            .await
            .unwrap()
            .expect("repo usage entry must exist after an agent report");
        assert_eq!(repo_usage.entity_type, "repo");
        assert_eq!(repo_usage.tokens_used_today, 1000);
        assert!((repo_usage.cost_today - 0.10).abs() < 1e-9);

        // Workspace total carries the same usage (roll-up).
        let ws_usage = state
            .budget_usages
            .get_usage(&super::workspace_key("ws-rollup"))
            .await
            .unwrap()
            .expect("workspace usage must exist");
        assert_eq!(ws_usage.tokens_used_today, 1000);
    }

    #[tokio::test]
    async fn repo_budget_limit_blocks_spawns() {
        let state = crate::mem::test_state();
        let mut repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-budgeted"),
            gyre_common::Id::new("ws-rb"),
            "budgeted",
            "/tmp/repo-budgeted",
            0,
        );
        repo.budget = Some(gyre_domain::BudgetConfig {
            max_tokens_per_day: Some(500),
            ..Default::default()
        });
        state.repos.create(&repo).await.unwrap();

        // Under the repo budget: allowed.
        assert!(super::check_repo_spawn_budget(state.as_ref(), &repo).await.is_ok());

        // At the limit: rejected (workspace check separately covers the
        // inherited case; this is the repo override firing).
        state
            .budget_usages
            .add_tokens_cost(
                &super::repo_key("repo-budgeted"),
                "repo",
                "repo-budgeted",
                0,
                500,
                0.0,
            )
            .await
            .unwrap();
        let err = super::check_repo_spawn_budget(state.as_ref(), &repo)
            .await
            .expect_err("repo budget exhausted must block spawns");
        assert!(err.contains("repo budget exceeded"), "got: {err}");

        // No override → inherits the workspace limit (no repo-level error).
        let mut unbounded = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-plain"),
            gyre_common::Id::new("ws-rb"),
            "plain",
            "/tmp/repo-plain",
            0,
        );
        unbounded.budget = None;
        assert!(super::check_repo_spawn_budget(state.as_ref(), &unbounded).await.is_ok());
    }

    #[tokio::test]
    async fn repo_concurrent_budget_override_blocks_spawns() {
        let state = crate::mem::test_state();
        let ws_id = gyre_common::Id::new("ws-rconc");
        let ws = Workspace::new(ws_id.clone(), gyre_common::Id::new("tenant-1"), "rconc", "rconc", 0);
        state.workspaces.create(&ws).await.unwrap();

        let mut repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-rconc"),
            ws_id.clone(),
            "rconc",
            "/tmp/repo-rconc",
            0,
        );
        repo.budget = Some(gyre_domain::BudgetConfig {
            max_concurrent_agents: Some(1),
            ..Default::default()
        });
        state.repos.create(&repo).await.unwrap();

        // No agents in the repo yet: allowed.
        assert!(super::check_repo_spawn_budget(state.as_ref(), &repo).await.is_ok());

        // One Active agent bound to the repo via its worktree (the worker
        // linkage path — workers carry no repo_id).
        let agent_id = gyre_common::Id::new("agent-rconc");
        let mut agent = Agent::new(agent_id.clone(), "worker", now_secs_static());
        agent.workspace_id = ws_id.clone();
        agent.status = AgentStatus::Active;
        state.agents.create(&agent).await.unwrap();
        let wt = gyre_domain::AgentWorktree::new(
            gyre_common::Id::new("wt-rconc"),
            agent_id.clone(),
            repo.id.clone(),
            None,
            "feat/rconc".to_string(),
            "/tmp/wt-rconc".to_string(),
            0,
        );
        state.worktrees.create(&wt).await.unwrap();

        let err = super::check_repo_spawn_budget(state.as_ref(), &repo)
            .await
            .expect_err("repo max_concurrent_agents=1 with one active agent must block spawns");
        assert!(
            err.contains("max_concurrent_agents=1"),
            "got: {err}"
        );

        // The agent completes → the slot frees and spawns are allowed again.
        let mut done = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        done.status = AgentStatus::Idle;
        state.agents.update(&done).await.unwrap();
        assert!(super::check_repo_spawn_budget(state.as_ref(), &repo).await.is_ok());
    }

    #[tokio::test]
    async fn agent_lifetime_enforced_from_workspace_config() {
        let (state, _ws_id, agent_id, task_id) =
            enforcement_fixture("ws-life-enc").await;
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-life-enc"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    max_agent_lifetime_secs: Some(3600),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Aged past the limit.
        let mut aged = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        aged.spawned_at = now_secs_static().saturating_sub(7200);
        state.agents.update(&aged).await.unwrap();

        super::enforce_agent_lifetimes(state.as_ref()).await;

        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "an agent past max_agent_lifetime_secs must be budget-killed"
        );
        // Budget-kill semantics: worktree preserved, task stays InProgress.
        let wts = state.worktrees.find_by_agent(&agent_id).await.unwrap();
        assert_eq!(wts.len(), 1, "lifetime kill must preserve the worktree");
        let task = state.tasks.find_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn repo_lifetime_override_tighter_than_workspace() {
        let (state, _ws_id, over_agent, _) =
            enforcement_fixture("ws-life-repo").await;
        // Workspace ceiling: 3600s.
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-life-repo"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    max_agent_lifetime_secs: Some(3600),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // The fixture agent's worktree lives in "ws-life-repo-repo": give
        // THAT repo a tighter 1800s override.
        let repo_id = gyre_common::Id::new("ws-life-repo-repo");
        let mut repo = gyre_domain::Repository::new(
            repo_id.clone(),
            gyre_common::Id::new("ws-life-repo"),
            "life-repo",
            "/tmp/repo-life-repo",
            0,
        );
        repo.budget = Some(gyre_domain::BudgetConfig {
            max_agent_lifetime_secs: Some(1800),
            ..Default::default()
        });
        state.repos.create(&repo).await.unwrap();

        // Aged 2000s: under the workspace ceiling (3600) but past the repo
        // override (1800) — the repo budget is the finer-grained control.
        let mut aged = state.agents.find_by_id(&over_agent).await.unwrap().unwrap();
        aged.spawned_at = now_secs_static().saturating_sub(2000);
        state.agents.update(&aged).await.unwrap();

        super::enforce_agent_lifetimes(state.as_ref()).await;

        let agent = state.agents.find_by_id(&over_agent).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "the repo lifetime override (1800s) must bind tighter than the workspace ceiling (3600s)"
        );

        // Control: an agent in a repo WITHOUT an override, aged the same
        // 2000s, stays Active under the 3600s workspace ceiling.
        let control_id = gyre_common::Id::new("ws-life-repo-control");
        let mut control = Agent::new(control_id.clone(), "control", now_secs_static());
        control.workspace_id = gyre_common::Id::new("ws-life-repo");
        control.status = AgentStatus::Active;
        control.spawned_at = now_secs_static().saturating_sub(2000);
        state.agents.create(&control).await.unwrap();
        let plain_repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-plain-life"),
            gyre_common::Id::new("ws-life-repo"),
            "plain-life",
            "/tmp/repo-plain-life",
            0,
        );
        state.repos.create(&plain_repo).await.unwrap();
        let control_wt = gyre_domain::AgentWorktree::new(
            gyre_common::Id::new("wt-plain-life"),
            control_id.clone(),
            plain_repo.id.clone(),
            None,
            "feat/plain".to_string(),
            "/tmp/wt-plain-life".to_string(),
            0,
        );
        state.worktrees.create(&control_wt).await.unwrap();

        super::enforce_agent_lifetimes(state.as_ref()).await;
        let agent = state.agents.find_by_id(&control_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Active,
            "an agent under the effective lifetime limit must keep running"
        );
    }

    #[tokio::test]
    async fn repo_lifetime_override_bounded_by_tenant_ceiling_workspace_unset() {
        // Review round 9af67822 F1(b) corner: the tenant sets the absolute
        // ceiling, the workspace leaves the dimension unset (inherited from
        // the tenant per platform-model.md §1), and a repo override ABOVE
        // the tenant ceiling exists in the store (set before the ceiling
        // was lowered). The repo override must not loosen the cascade:
        // effective limit = min(repo override, effective workspace limit).
        let (state, _ws_id, agent_id, _task_id) =
            enforcement_fixture("ws-life-tenant").await;
        // The fixture sets only max_tokens_per_day on the workspace — the
        // lifetime dimension is unset there and inherits the tenant's.
        state
            .budget_configs
            .set_config(
                super::tenant_key(),
                &gyre_domain::BudgetConfig {
                    max_agent_lifetime_secs: Some(1800),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Stale repo override above the tenant ceiling.
        let repo_id = gyre_common::Id::new("ws-life-tenant-repo");
        let mut repo = gyre_domain::Repository::new(
            repo_id.clone(),
            gyre_common::Id::new("ws-life-tenant"),
            "life-tenant-repo",
            "/tmp/repo-life-tenant",
            0,
        );
        repo.budget = Some(gyre_domain::BudgetConfig {
            max_agent_lifetime_secs: Some(3600),
            ..Default::default()
        });
        state.repos.create(&repo).await.unwrap();

        // Aged 2000s: past the tenant ceiling (1800) even though the repo
        // override says 3600 — the override cannot loosen the cascade.
        let mut aged = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        aged.spawned_at = now_secs_static().saturating_sub(2000);
        state.agents.update(&aged).await.unwrap();

        super::enforce_agent_lifetimes(state.as_ref()).await;

        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "a repo lifetime override above the tenant ceiling must stay bounded: \
             effective limit = min(3600, 1800) = 1800, agent aged 2000s"
        );
    }

    #[tokio::test]
    async fn repo_lifetime_override_rebinds_after_workspace_reduction() {
        // A repo override legitimately at 3000s under a 3600s workspace
        // ceiling binds as-is — until the workspace budget is reduced
        // below it. The lifetime dimension recomputes the effective limit
        // live on every enforcement pass (the same semantics the
        // token/cost/concurrent dimensions honor), so a later reduction
        // to 1800s kills an agent aged past 1800s even though the repo
        // override still says 3000s.
        let (state, _ws_id, agent_id, _task_id) =
            enforcement_fixture("ws-life-reduce").await;
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-life-reduce"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    max_agent_lifetime_secs: Some(3600),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let repo_id = gyre_common::Id::new("ws-life-reduce-repo");
        let mut repo = gyre_domain::Repository::new(
            repo_id.clone(),
            gyre_common::Id::new("ws-life-reduce"),
            "life-reduce-repo",
            "/tmp/repo-life-reduce",
            0,
        );
        repo.budget = Some(gyre_domain::BudgetConfig {
            max_agent_lifetime_secs: Some(3000),
            ..Default::default()
        });
        state.repos.create(&repo).await.unwrap();

        // Aged 2000s: under min(3000, 3600) = 3000 — keeps running.
        let mut aged = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        aged.spawned_at = now_secs_static().saturating_sub(2000);
        state.agents.update(&aged).await.unwrap();
        super::enforce_agent_lifetimes(state.as_ref()).await;
        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Active,
            "an agent aged 2000s is under the legitimate repo override (3000s)"
        );

        // Workspace budget reduced below the repo override: the effective
        // limit is now min(3000, 1800) = 1800 — the reduction binds.
        state
            .budget_configs
            .set_config(
                &super::workspace_key("ws-life-reduce"),
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    max_agent_lifetime_secs: Some(1800),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        super::enforce_agent_lifetimes(state.as_ref()).await;

        let agent = state.agents.find_by_id(&agent_id).await.unwrap().unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Stopped,
            "a workspace budget reduction below the repo override must bind: \
             effective limit = min(3000, 1800) = 1800, agent aged 2000s"
        );
        // Budget-kill semantics: worktree preserved.
        let wts = state.worktrees.find_by_agent(&agent_id).await.unwrap();
        assert_eq!(wts.len(), 1, "lifetime kill must preserve the worktree");
    }

    #[tokio::test]
    async fn workspace_lifetime_cascade_rejects_exceeding_tenant() {
        let state = crate::mem::test_state();
        state
            .budget_configs
            .set_config(
                super::tenant_key(),
                &gyre_domain::BudgetConfig {
                    max_agent_lifetime_secs: Some(3600),
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
                    .uri("/api/v1/workspaces/ws-life/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_agent_lifetime_secs":7200}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
