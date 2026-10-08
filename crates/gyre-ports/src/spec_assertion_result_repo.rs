//! Port trait for persisted spec assertion results (system-explorer.md §9).
//!
//! The post-push knowledge-graph check parses `<!-- gyre:assert ... -->`
//! comments from specs, evaluates them against the fresh graph, and stores
//! one result record per assertion, keyed by (repo_id, spec_path, line) —
//! the latest push's result replaces any earlier one for that assertion.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::SpecAssertionResult;

#[async_trait]
pub trait SpecAssertionResultRepository: Send + Sync {
    /// Persist a batch of results for one spec. Any prior rows for the same
    /// (repo_id, spec_path) are replaced, so the stored set always reflects
    /// the latest push's check.
    async fn save_results(&self, results: &[SpecAssertionResult]) -> Result<()>;

    /// List the stored results for one spec in a repo, ordered by line.
    async fn list_by_spec(&self, repo_id: &str, spec_path: &str) -> Result<Vec<SpecAssertionResult>>;

    /// Delete all stored results for a repo (e.g. when the repo is removed).
    async fn delete_by_repo(&self, repo_id: &str) -> Result<()>;
}
