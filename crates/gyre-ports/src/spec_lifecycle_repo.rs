use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::SpecLifecycleConfig;

/// Per-repo spec lifecycle configuration (spec-lifecycle.md §Configuration).
///
/// Absent rows mean "defaults" — `get_for_repo` returns
/// [`SpecLifecycleConfig::default()`], never an error, so the post-receive
/// hook degrades to default behavior rather than skipping processing.
#[async_trait]
pub trait SpecLifecycleRepository: Send + Sync {
    /// Get the spec lifecycle config for a repo. Returns defaults when
    /// none has been configured.
    async fn get_for_repo(&self, repo_id: &str) -> Result<SpecLifecycleConfig>;
    /// Store the spec lifecycle config for a repo.
    async fn set_for_repo(&self, repo_id: &str, config: SpecLifecycleConfig) -> Result<()>;
}
