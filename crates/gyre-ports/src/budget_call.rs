use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::BudgetCallRecord;

/// Persistence port for per-call LLM budget audit records (platform-model.md §5
/// Budget Tracking).
///
/// Append-only log of every LLM invocation — agent-initiated (`agent_run`) and
/// user-initiated (`llm_query`: briefing/ask, explorer-views/generate,
/// specs/assist). Written by the budget recording helpers; read for reporting
/// and retention.
#[async_trait]
pub trait BudgetCallRepository: Send + Sync {
    /// Append a call record. Insert-only; the record id is unique.
    async fn save(&self, record: &BudgetCallRecord) -> Result<()>;

    /// List call records for a workspace with `timestamp >= since`, newest
    /// first, at most `limit` rows.
    async fn list_by_workspace(
        &self,
        workspace_id: &str,
        since: u64,
        limit: i64,
    ) -> Result<Vec<BudgetCallRecord>>;
}
