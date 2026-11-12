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

// ── GET /api/v1/workspaces/{id}/budget/by-team  (user-management.md §Team
//    Management — budget reporting: aggregate budget usage by team) ─────────

#[derive(Debug, Serialize)]
pub struct TeamBudgetReport {
    pub team_id: String,
    pub slug: String,
    pub name: String,
    pub member_count: usize,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize)]
pub struct TeamBudgetSummary {
    pub workspace_id: String,
    pub teams: Vec<TeamBudgetReport>,
    /// Usage not attributable to any team member: records with no agent, or
    /// agents with no spawning user (user-initiated queries).
    pub unattributed: TeamBudgetReport,
}

/// GET /api/v1/workspaces/{id}/budget/by-team — aggregate the workspace's
/// LLM budget usage by team membership.
///
/// Attribution: each `BudgetCallRecord` caused by an agent is attributed to
/// the agent's spawning user; a record counts toward every team that user
/// belongs to (that is what "aggregate by team membership" means when a
/// user sits on multiple teams). Records with no attributable user are
/// reported as `unattributed` rather than silently dropped.
///
/// Scoped to the caller's tenant like every workspace read.
pub async fn budget_by_team(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<TeamBudgetSummary>, ApiError> {
    let ws = state
        .workspaces
        .find_by_id(&Id::new(id.clone()))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if !auth.is_system_principal() && ws.tenant_id.to_string() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "cross-tenant workspace access denied".to_string(),
        ));
    }

    let teams = state
        .teams
        .list_by_workspace(&ws.id)
        .await
        .map_err(ApiError::Internal)?;
    let mut reports = Vec::with_capacity(teams.len());
    let mut member_sets = Vec::with_capacity(teams.len());
    for team in &teams {
        let members = state
            .teams
            .list_members(&team.id)
            .await
            .map_err(ApiError::Internal)?;
        member_sets.push(
            members
                .iter()
                .map(|m| m.user_id.to_string())
                .collect::<std::collections::HashSet<String>>(),
        );
        reports.push(TeamBudgetReport {
            team_id: team.id.to_string(),
            slug: team.slug.clone(),
            name: team.name.clone(),
            member_count: members.len(),
            calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
        });
    }
    let mut unattributed = TeamBudgetReport {
        team_id: String::new(),
        slug: "unattributed".to_string(),
        name: "Unattributed (no team member attribution)".to_string(),
        member_count: 0,
        calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: 0.0,
    };

    // Full audit trail since the epoch, capped at the port's row limit.
    let records = state
        .budget_calls
        .list_by_workspace(ws.id.as_str(), 0, 100_000)
        .await
        .map_err(ApiError::Internal)?;

    for record in &records {
        let report_target = |r: &mut TeamBudgetReport| {
            r.calls += 1;
            r.input_tokens += record.input_tokens;
            r.output_tokens += record.output_tokens;
            r.cost_usd += record.cost_usd;
        };
        // Attribute to the agent's spawning user, when there is one.
        let spawned_by = match &record.agent_id {
            Some(aid) => state
                .agents
                .find_by_id(aid)
                .await
                .ok()
                .flatten()
                .and_then(|a| a.spawned_by),
            None => None,
        };
        match spawned_by {
            Some(uid) => {
                let uid = uid.to_string();
                let mut attributed = false;
                for (report, members) in reports.iter_mut().zip(member_sets.iter()) {
                    if members.contains(&uid) {
                        report_target(report);
                        attributed = true;
                    }
                }
                if !attributed {
                    report_target(&mut unattributed);
                }
            }
            None => report_target(&mut unattributed),
        }
    }

    Ok(Json(TeamBudgetSummary {
        workspace_id: ws.id.to_string(),
        teams: reports,
        unattributed,
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
/// trail and the aggregate counters can never diverge.
///
/// Best-effort persistence: a failed append or counter increment is logged
/// and never fails the caller's request (the LLM answer is already produced;
/// usage recording must not discard it).
pub async fn record_llm_call_usage(state: &AppState, usage: &LlmCallUsage) {
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

    maybe_notify_budget_threshold(state, usage).await;
}

/// user-management.md §Notification Entity: `AgentBudgetWarning` (≥ 80% of a
/// daily limit, High — needs attention soon) and `AgentBudgetExhausted`
/// (≥ 100%, Urgent — work is stopped until the period resets).
///
/// Recipients follow the spec's "Who Gets Notified" table: the agent's
/// spawning user plus workspace Admins (warning) / the workspace Owner
/// (exhausted). Emitted at most once per user per budget period via
/// `has_active_notification` so repeated usage reports within the same
/// period do not spam the inbox. Best-effort: failures are logged, never
/// propagated (usage recording must not fail the caller's request).
async fn maybe_notify_budget_threshold(state: &AppState, usage: &LlmCallUsage) {
    let ws_key = workspace_key(&usage.workspace_id.to_string());

    let Ok(Some(config)) = state.budget_configs.get_config(&ws_key).await else {
        return; // no limits configured — nothing to warn about
    };
    let Ok(Some(ws_usage)) = state.budget_usages.get_usage(&ws_key).await else {
        return;
    };

    // Highest utilization across the configured daily limits.
    let token_pct = config
        .max_tokens_per_day
        .filter(|max| *max > 0)
        .map(|max| ws_usage.tokens_used_today as f64 / max as f64);
    let cost_pct = config
        .max_cost_per_day
        .filter(|max| *max > 0.0)
        .map(|max| ws_usage.cost_today / max);
    let pct = match (token_pct, cost_pct) {
        (Some(t), Some(c)) => Some(t.max(c)),
        (Some(t), None) => Some(t),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    };
    let Some(pct) = pct else {
        return;
    };

    let exhausted = pct >= 1.0;
    let warning = !exhausted && pct >= 0.8;
    if !exhausted && !warning {
        return;
    }

    // The tenant scope comes from the workspace lookup — when it cannot be
    // resolved there is no scoperable tenant, so skip rather than fabricate
    // one (check-fabricated-scope-defaults.sh).
    let Ok(Some(ws)) = state.workspaces.find_by_id(&usage.workspace_id).await else {
        return;
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
    if let Some(agent_id) = usage.agent_id.as_ref() {
        if let Ok(Some(agent)) = state.agents.find_by_id(agent_id).await {
            if let Some(spawned_by) = agent.spawned_by {
                recipients.push(gyre_common::Id::new(spawned_by));
            }
        }
    }
    if let Ok(members) = state
        .workspace_memberships
        .list_by_workspace(&usage.workspace_id)
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
    let entity_ref = usage.workspace_id.to_string();
    let since = ws_usage.period_start as i64;
    for user_id in recipients {
        let already_notified = state
            .notifications
            .has_active_notification(
                &usage.workspace_id,
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
            "workspace_id": usage.workspace_id.as_str(),
            "usage_pct": (pct * 100.0).round(),
            "tokens_used_today": ws_usage.tokens_used_today,
            "max_tokens_per_day": config.max_tokens_per_day,
            "cost_today": ws_usage.cost_today,
            "max_cost_per_day": config.max_cost_per_day,
            "agent_id": usage.agent_id.as_ref().map(|a| a.as_str()),
        })
        .to_string();
        crate::notifications::notify_rich(
            state,
            usage.workspace_id.clone(),
            user_id,
            ntype.clone(),
            title.clone(),
            &tenant_id,
            Some(body),
            Some(entity_ref.clone()),
            usage.repo_id.as_ref().map(|r| r.to_string()),
            "budget",
            None,
        )
        .await;
    }
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

    /// user-management.md §Team Management — budget reporting: aggregate
    /// budget usage by team. Attribution flows through real records: an
    /// agent-caused BudgetCallRecord attributes to the agent's spawning
    /// user, then to every team that user belongs to. Records with no
    /// attributable user land in `unattributed` rather than vanishing.
    #[tokio::test]
    async fn budget_by_team_aggregates_usage_through_membership() {
        use gyre_common::Id;
        use gyre_domain::{BudgetCallRecord, Team, TeamMembership, Workspace};

        let state = crate::mem::test_state();
        let app = crate::build_router(state.clone());

        let ws = Workspace {
            id: Id::new("ws-bt"),
            tenant_id: Id::new("tenant-1"),
            name: "Budget Teams WS".to_string(),
            slug: "budget-teams-ws".to_string(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Guided,
            llm_model: None,
            created_at: 0,
            compute_target_id: None,
        };
        state.workspaces.create(&ws).await.unwrap();

        // Team with two members; a third user sits on no team.
        state
            .teams
            .create(&Team::new(
                Id::new("t-bt"),
                Id::new("ws-bt"),
                "Platform",
                "platform",
                Id::new("u-lead"),
                0,
            ))
            .await
            .unwrap();
        for uid in ["u-lead", "u-dev"] {
            state
                .teams
                .add_member(&TeamMembership::new(Id::new("t-bt"), Id::new(uid), 0))
                .await
                .unwrap();
        }

        // Agents spawned by the team members and by the teamless user.
        let mut agent_dev = gyre_domain::Agent::new(Id::new("a-dev"), "dev agent", 0);
        agent_dev.spawned_by = Some("u-dev".to_string());
        state.agents.create(&agent_dev).await.unwrap();
        let mut agent_loner = gyre_domain::Agent::new(Id::new("a-loner"), "loner agent", 0);
        agent_loner.spawned_by = Some("u-loner".to_string());
        state.agents.create(&agent_loner).await.unwrap();

        let call =
            |id: &str, agent: Option<&str>, input: u64, output: u64, cost: f64| BudgetCallRecord {
                id: Id::new(id),
                tenant_id: Id::new("tenant-1"),
                workspace_id: Id::new("ws-bt"),
                repo_id: None,
                agent_id: agent.map(Id::new),
                task_id: None,
                usage_type: if agent.is_some() {
                    "agent_run"
                } else {
                    "llm_query"
                }
                .to_string(),
                input_tokens: input,
                output_tokens: output,
                cost_usd: cost,
                model: "test-model".to_string(),
                timestamp: 1,
            };
        // u-dev's agent: 2 calls, 300 tokens total, 0.03.
        state
            .budget_calls
            .save(&call("c-1", Some("a-dev"), 100, 50, 0.01))
            .await
            .unwrap();
        state
            .budget_calls
            .save(&call("c-2", Some("a-dev"), 100, 50, 0.02))
            .await
            .unwrap();
        // u-loner's agent: 1 call, unattributed to any team.
        state
            .budget_calls
            .save(&call("c-3", Some("a-loner"), 10, 5, 0.001))
            .await
            .unwrap();
        // A user-initiated query with no agent: unattributed.
        state
            .budget_calls
            .save(&call("c-4", None, 7, 3, 0.0005))
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-bt/budget/by-team")
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

        let teams = v["teams"].as_array().expect("teams array");
        assert_eq!(teams.len(), 1, "one team in the workspace: {teams:?}");
        let team = &teams[0];
        assert_eq!(team["slug"], "platform");
        assert_eq!(team["member_count"], 2);
        assert_eq!(team["calls"], 2, "u-dev's two agent calls: {team:?}");
        assert_eq!(team["input_tokens"], 200);
        assert_eq!(team["output_tokens"], 100);
        assert_eq!(team["cost_usd"], 0.03);

        let unattr = &v["unattributed"];
        assert_eq!(unattr["calls"], 2, "loner + agentless records: {unattr:?}");
        assert_eq!(unattr["input_tokens"], 17);
        assert_eq!(unattr["output_tokens"], 8);
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
}
