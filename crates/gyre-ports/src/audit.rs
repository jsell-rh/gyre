use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::{AuditEvent, AuditOutcome};
use serde::Deserialize;

/// Filter dimensions for querying audit events (observability.md §Audit Event
/// Schema). Every field is optional; `None` means "no constraint".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuditQueryFilter {
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub resource_type: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: usize,
}

/// Port for recording and querying audit events.
#[async_trait]
pub trait AuditRepository: Send + Sync {
    async fn record(&self, event: &AuditEvent) -> Result<()>;

    async fn query(&self, filter: &AuditQueryFilter) -> Result<Vec<AuditEvent>>;

    /// Total count of all audit events.
    async fn count(&self) -> Result<u64>;

    /// Returns (event_type, count) pairs across all events.
    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>>;

    /// Returns events with id > after_id ordered by timestamp ascending.
    /// Used by SIEM forwarding to stream new events.
    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>>;

    /// Hard-delete events with timestamp < cutoff_secs. Returns the number of rows purged.
    /// Used by the nightly retention job (business-continuity.md §5).
    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64>;
}
