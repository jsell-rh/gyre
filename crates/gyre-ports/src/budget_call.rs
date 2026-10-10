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
}
