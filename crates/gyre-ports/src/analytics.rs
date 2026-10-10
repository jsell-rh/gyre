use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};

/// Filter for querying analytics events (analytics.md §Query Parameters).
///
/// All fields are optional; `None` means no constraint on that dimension.
/// `event_name` supports exact match or trailing-`*` prefix match
/// (e.g. `mr.*`), per spec.
#[derive(Debug, Clone, Default)]
pub struct AnalyticsQueryFilter {
    /// Exact event name, or prefix match when it ends with `*`.
    pub event_name: Option<String>,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    /// Inclusive lower bound (unix seconds).
    pub since: Option<u64>,
    /// Inclusive upper bound (unix seconds).
    pub until: Option<u64>,
    /// Max results (server enforces a cap).
    pub limit: usize,
}

impl AnalyticsQueryFilter {
    pub fn new() -> Self {
        Self {
            limit: 100,
            ..Default::default()
        }
    }

    /// Split `event_name` into a SQL LIKE pattern: `mr.*` → `mr.%`,
    /// exact otherwise. Returns `None` when no filter is set.
    pub fn event_name_like(&self) -> Option<String> {
        self.event_name.as_ref().map(|name| {
            if let Some(prefix) = name.strip_suffix('*') {
                format!("{}%", prefix)
            } else {
                name.clone()
            }
        })
    }
}

#[async_trait]
pub trait AnalyticsRepository: Send + Sync {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()>;
    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>>;
    /// Query with the full spec filter set (analytics.md §Query Parameters):
    /// event_name (exact or trailing-`*` prefix), agent/user/workspace/repo
    /// scope ids, since/until bounds, limit.
    async fn query_filtered(&self, filter: &AnalyticsQueryFilter) -> Result<Vec<AnalyticsEvent>>;
    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64>;
    /// Returns (date_str, count) pairs grouped by calendar day (UTC).
    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>>;

    /// Hard-delete events with timestamp < cutoff_secs. Returns the number of rows purged.
    /// Used by the nightly retention job (business-continuity.md §5).
    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64>;
}

#[async_trait]
pub trait CostRepository: Send + Sync {
    async fn record(&self, entry: &CostEntry) -> Result<()>;
    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>>;
    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>>;
    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64>;
    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64>;
}
