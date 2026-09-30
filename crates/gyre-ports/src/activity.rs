use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::ActivityEvent;

pub struct ActivityQuery {
    pub since: Option<u64>,
    pub limit: Option<usize>,
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
}

#[async_trait]
pub trait ActivityRepository: Send + Sync {
    async fn append(&self, event: &ActivityEvent) -> Result<()>;
    async fn query(&self, q: &ActivityQuery) -> Result<Vec<ActivityEvent>>;

    /// Hard-delete events with timestamp < cutoff_secs. Returns the number of rows purged.
    /// Used by the nightly retention job (business-continuity.md §5).
    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64>;
}
