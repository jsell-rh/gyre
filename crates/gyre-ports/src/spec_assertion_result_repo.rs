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

    /// Delete all stored results for one spec in a repo. Called when the
    /// latest push's check found no assertions for the spec — either the
    /// assertions were removed from the spec or the spec file was deleted —
    /// so stale rows from an earlier push do not linger as the spec's
    /// "latest check" state.
    async fn delete_by_spec(&self, repo_id: &str, spec_path: &str) -> Result<()>;

    /// List the stored results for one spec in a repo, ordered by line.
    async fn list_by_spec(&self, repo_id: &str, spec_path: &str) -> Result<Vec<SpecAssertionResult>>;

    /// List the distinct spec paths with stored results for a repo. Used by
    /// the post-push check to sweep results for specs that no longer exist
    /// in the pushed tree (deleted or renamed spec files).
    async fn list_spec_paths(&self, repo_id: &str) -> Result<Vec<String>>;

    /// Delete all stored results for a repo (e.g. when the repo is removed).
    async fn delete_by_repo(&self, repo_id: &str) -> Result<()>;
}
