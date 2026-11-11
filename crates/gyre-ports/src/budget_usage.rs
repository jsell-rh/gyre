use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::BudgetUsage;

/// Persistence port for real-time budget usage snapshots (M22.2).
///
/// Complementary to `BudgetRepository` (which handles `BudgetConfig` limits).
/// Usage counters are keyed by the same `entity_key` format:
/// `"workspace:{id}"` or `"tenant:global"`.
///
/// Atomic operations (increment/decrement/add) are required to prevent
/// race conditions under concurrent agent spawns.
#[async_trait]
pub trait BudgetUsageRepository: Send + Sync {
    /// Upsert a budget usage snapshot for `entity_key`.
    async fn set_usage(&self, entity_key: &str, usage: &BudgetUsage) -> Result<()>;

    /// Retrieve usage snapshot. Returns `None` if no usage recorded yet.
    async fn get_usage(&self, entity_key: &str) -> Result<Option<BudgetUsage>>;

    /// Delete usage snapshot (e.g. for cleanup).
    async fn delete_usage(&self, entity_key: &str) -> Result<()>;

    /// List all (entity_key, BudgetUsage) pairs.
    async fn list_all_usage(&self) -> Result<Vec<(String, BudgetUsage)>>;

    /// Atomically increment `active_agents` by 1.
    /// Creates the entry from `entity_type`, `entity_id`, and `now` if absent.
    async fn increment_active(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
    ) -> Result<BudgetUsage>;

    /// Atomically decrement `active_agents` by 1 (saturating at 0).
    /// No-op if the entry does not exist.
    async fn decrement_active(&self, entity_key: &str) -> Result<()>;

    /// Atomically add `tokens` and `cost_usd` to the usage counters.
    /// Creates the entry if absent.
    async fn add_tokens_cost(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
        tokens: u64,
        cost_usd: f64,
    ) -> Result<()>;

    /// Atomically add `minutes` of agent wall-clock compute time to the
    /// usage entry (agent-runtime.md §4 What's Tracked: `compute_minutes`,
    /// spawn → complete/kill). Creates the entry if absent. Tracking-only —
    /// no limit field consumes it.
    async fn add_compute_minutes(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
        minutes: u64,
    ) -> Result<()>;
}
