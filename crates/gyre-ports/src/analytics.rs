use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};

#[async_trait]
pub trait AnalyticsRepository: Send + Sync {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()>;
    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>>;
    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64>;
    /// Returns (date_str, count) pairs grouped by calendar day (UTC).
    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>>;
}

#[async_trait]
pub trait CostRepository: Send + Sync {
    async fn record(&self, entry: &CostEntry) -> Result<()>;
    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>>;
    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>>;
    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64>;
    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64>;
}
