use gyre_common::Id;

/// Budget configuration for a workspace, tenant, or repo level.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BudgetConfig {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
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
