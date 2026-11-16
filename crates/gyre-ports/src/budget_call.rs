use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::BudgetCallRecord;

/// Append-only persistence port for per-call LLM budget audit records
/// (platform-model.md §Budget Tracking).
///
/// Every LLM invocation — agent-initiated (`usage_type = "agent_run"`) or
/// user-initiated (`"llm_query"`: briefing/ask, explorer-views/generate,
/// specs/assist) — appends one record. The aggregate counters in
/// `BudgetUsageRepository` are the enforcement input; this table is the
/// per-call audit trail behind them and the basis for later reporting and
/// retention.
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
#[async_trait]
pub trait BudgetCallRepository: Send + Sync {
    /// Append a per-call record. Insert-only; duplicate `id` is an error.
    async fn save(&self, record: &BudgetCallRecord) -> Result<()>;

    /// List records for a workspace, newest first, filtered to
    /// `timestamp >= since`. `limit` caps the number of rows returned.
    async fn list_by_workspace(
        &self,
        workspace_id: &str,
        since: u64,
        limit: i64,
    ) -> Result<Vec<BudgetCallRecord>>;

    /// List records across ALL workspaces and tenants (deployment-wide),
    /// newest first, filtered to `timestamp >= since`. `limit` caps the
    /// number of rows returned. Input to the hourly rolling-24h-window
    /// budget reset job (agent-runtime.md §4 Budget Reset), which
    /// recomputes the aggregate counters from the per-call records
    /// inside the window. Deliberately NOT scoped to the storage
    /// handle's tenant: the reset job runs on the base handle and
    /// resyncs every workspace's counters plus the storage-wide
    /// `tenant:global` ceiling, so a tenant filter here would drop
    /// other tenants' records and zero their usage. Every adapter
    /// (mem, SQLite, PostgreSQL) must implement this unscoped.
    async fn list_since(&self, since: u64, limit: i64) -> Result<Vec<BudgetCallRecord>>;
}
