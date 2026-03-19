use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::{BranchInfo, CommitInfo, DiffResult};

#[async_trait]
pub trait GitOpsPort: Send + Sync {
    /// Initialize a new bare repository at the given path.
    async fn init_bare(&self, path: &str) -> Result<()>;

    /// List branches with their head commit SHA.
    async fn list_branches(&self, repo_path: &str) -> Result<Vec<BranchInfo>>;

    /// Get commit log for a branch (most recent first).
    async fn commit_log(
        &self,
        repo_path: &str,
        branch: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>>;

    /// Get diff between two refs (commits, branches, tags).
    async fn diff(&self, repo_path: &str, from: &str, to: &str) -> Result<DiffResult>;

    /// Check if a path is a valid git repository.
    async fn is_repo(&self, path: &str) -> Result<bool>;

    /// Check if source branch can be merged into target without conflicts.
    async fn can_merge(&self, repo_path: &str, source: &str, target: &str) -> Result<bool>;
}
