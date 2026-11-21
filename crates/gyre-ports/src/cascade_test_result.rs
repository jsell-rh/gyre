//! Port trait for durable cascade test result persistence
//! (dependency-graph.md §Cascade Testing).
//!
//! Cascade test outcomes must survive a server restart: the record is the
//! durable counterpart of the one-shot `cascade_test_passed`/`cascade_test_failed`
//! workspace events. Implemented by the mem, Diesel SQLite, and PostgreSQL
//! adapters with an identical contract.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::CascadeTestResult;

/// Port for persisting cascade test results.
///
/// Contract (enforced identically by every adapter):
/// - `record` is an upsert keyed on `cascade_task_id`: at most ONE result row
///   exists per cascade test task, and re-reporting the same task (agent
///   retry, duplicate completion) REPLACES the row with the latest report —
///   it never duplicates rows.
/// - `find_by_task` returns the latest recorded result for the task, `None`
///   if the task has no reported result.
/// - List methods return results ordered newest-first (`recorded_at`
///   descending).
#[async_trait]
pub trait CascadeTestResultRepository: Send + Sync {
    /// Persist a result, replacing any existing row for the same
    /// `cascade_task_id` (one result per cascade test task — last report
    /// wins, so retries never duplicate rows).
    async fn record(&self, result: &CascadeTestResult) -> Result<()>;

    /// The recorded result for a cascade test task, if one was reported.
    async fn find_by_task(&self, task_id: &Id) -> Result<Option<CascadeTestResult>>;

    /// All results for a dependent repo (its tests ran against a merged
    /// upstream), newest first.
    async fn list_by_dependent_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>>;

    /// All results for merges in a repo (it was the merged upstream that
    /// triggered the cascade runs), newest first.
    async fn list_by_merged_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>>;
}
