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
}
