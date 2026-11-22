use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};

/// Filter for querying analytics events (analytics.md §Query Parameters).
///
/// All fields are optional; `None` means no constraint on that dimension.
/// `event_name` supports exact match or trailing-`*` prefix match
/// (e.g. `mr.*`), per spec. Prefix matching is LITERAL and case-sensitive
/// on every adapter (task-146 review R4: `MR.*` must not match
/// `mr.merged`, and `%`/`_` inside the prefix are ordinary characters,
/// not wildcards) — the mem adapter's `starts_with`, SQLite's
/// `substr(...) = ...` under BINARY collation, and Postgres's escaped
/// case-sensitive LIKE all implement exactly this contract.
#[derive(Debug, Clone)]
pub struct AnalyticsQueryFilter {
    /// Exact event name, or literal case-sensitive prefix match when it
    /// ends with `*`.
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

impl Default for AnalyticsQueryFilter {
    /// Manual impl: `limit` defaults to the same value as [`new`](Self::new)
    /// and the REST query handler (100). A derived Default would set the
    /// `usize` to 0, and every adapter's `query_filtered` truncates to
    /// `limit` — so `..Default::default()` constructions would silently
    /// match nothing (task-295: this made the preview.analytics count
    /// probe read 0 and turned sibling "assert-empty" tests vacuous).
    fn default() -> Self {
        Self {
            event_name: None,
            agent_id: None,
            user_id: None,
            workspace_id: None,
            repo_id: None,
            since: None,
            until: None,
            limit: 100,
        }
    }
}

/// Grouping dimension for `AnalyticsRepository::count_grouped`
/// (analytics.md §Query Parameters: `group_by`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsGroupBy {
    EventName,
    AgentId,
    WorkspaceId,
    /// UTC calendar day, keyed `YYYY-MM-DD` (same key format as
    /// `aggregate_by_day`).
    Day,
}

impl AnalyticsQueryFilter {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
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
    /// Count events matching `filter` grouped by `group_by`, aggregated
    /// server-side over the FULL filtered set (task-146 review F1: the
    /// event-level `limit` must NOT truncate the input to the
    /// aggregation — grouped totals must stay consistent with `count`).
    /// Here `filter.limit` bounds the number of returned GROUPS instead.
    /// Returns `(group_key, count)` pairs ordered by group key ascending;
    /// NULL agent_id/workspace_id group under `"(none)"`.
    async fn count_grouped(
        &self,
        filter: &AnalyticsQueryFilter,
        group_by: AnalyticsGroupBy,
    ) -> Result<Vec<(String, u64)>>;
    /// Returns (date_str, count) pairs grouped by calendar day (UTC).
    /// `event_name` is an exact-match filter; `None` aggregates across
    /// ALL event names — the total daily event volume the Analytics
    /// Dashboard bar chart renders (analytics.md §Analytics Dashboard:
    /// "daily event volume for the last 30 days"). The spec query form
    /// `GET /daily?event_name=&days=30` (empty value) is `None`.
    async fn aggregate_by_day(
        &self,
        event_name: Option<&str>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// task-295 verification repair: a derived `Default` set `limit` to 0,
    /// and every adapter's `query_filtered` truncates to `filter.limit`,
    /// so `..Default::default()` constructions silently matched nothing
    /// (the try-it analytics count probe read 0 events it had just
    /// recorded). The default must stay a real result cap, identical to
    /// `new()` and the REST query handler's default.
    #[test]
    fn default_filter_keeps_a_nonzero_result_cap() {
        let f = AnalyticsQueryFilter::default();
        assert!(f.limit > 0, "default limit must not zero out query results");
        assert_eq!(f.limit, AnalyticsQueryFilter::new().limit);
        assert_eq!(f.event_name, None);
        assert_eq!(f.agent_id, None);
        assert_eq!(f.user_id, None);
        assert_eq!(f.workspace_id, None);
        assert_eq!(f.repo_id, None);
        assert_eq!(f.since, None);
        assert_eq!(f.until, None);
    }
}
