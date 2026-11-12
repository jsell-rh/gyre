use gyre_common::Id;

/// Action taken when a budget threshold is crossed
/// (platform-model.md §5 Resource Governance → Enforcement Behavior).
/// Configurable per level via `BudgetConfig::enforcement_action`;
/// `None` means the spec default (80% → Warn, 100% → GracefulStop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetEnforcementAction {
    /// Log warning, continue.
    Warn,
    /// Send BudgetExhausted message, grace period, then kill.
    GracefulStop,
    /// Immediate termination.
    HardKill,
    /// Pause merge queue until budget is reviewed.
    QueuePause,
}

/// Budget configuration for a workspace, tenant, or repo level.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BudgetConfig {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
    /// Per-level override for the action taken when utilization reaches
    /// 100% of a limit. `None` = spec default (GracefulStop with a 60s
    /// grace period). The 80% threshold is always `Warn`.
    #[serde(default)]
    pub enforcement_action: Option<BudgetEnforcementAction>,
}

/// Real-time budget usage snapshot for an entity (workspace or tenant).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetUsage {
    pub entity_type: String,
    pub entity_id: Id,
    pub tokens_used_today: u64,
    pub cost_today: f64,
    pub active_agents: u32,
    pub period_start: u64,
}

/// Per-call LLM budget audit record. Appended for every LLM invocation —
/// both agent-initiated and user-initiated (briefing, explorer, spec assist).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetCallRecord {
    pub id: Id,
    pub tenant_id: Id,
    pub workspace_id: Id,
    /// None for user-initiated LLM queries that have no repo context.
    pub repo_id: Option<Id>,
    /// None for user-initiated LLM queries.
    pub agent_id: Option<Id>,
    /// None when not associated with a specific task.
    pub task_id: Option<Id>,
    /// Distinguishes cost entry sources, e.g. "agent_run", "llm_query".
    pub usage_type: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub model: String,
    pub timestamp: u64,
}
