use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::AuditEvent;

/// Port for recording and querying audit events.
#[async_trait]
pub trait AuditRepository: Send + Sync {
    async fn record(&self, event: &AuditEvent) -> Result<()>;

    async fn query(
        &self,
        agent_id: Option<&str>,
        event_type: Option<&str>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>>;

    /// Total count of all audit events.
    async fn count(&self) -> Result<u64>;

    /// Returns (event_type, count) pairs across all events.
    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>>;

    /// Returns events with id > after_id ordered by timestamp ascending.
    /// Used by SIEM forwarding to stream new events.
    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>>;
}
