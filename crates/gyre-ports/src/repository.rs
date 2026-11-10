use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Repository;

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RepoRepository: Send + Sync {
    /// Fails if a repository with the same `id`, or the same `(workspace_id,
    /// name)`, already exists — the pair maps to one bare-repo path
    /// (`{repos_root}/{workspace_id}/{name}.git`), so a duplicate row would
    /// alias another repo's storage (SQLite enforces it via the
    /// `idx_repos_workspace_name` unique index).
    async fn create(&self, repo: &Repository) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>>;
    async fn list(&self) -> Result<Vec<Repository>>;
    async fn update(&self, repo: &Repository) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Repository>>;
    /// Find a repository by name within a workspace.
    /// Required for git URL resolution: /git/:workspace_slug/:repo_name/*.
    async fn find_by_name_and_workspace(
        &self,
        workspace_id: &Id,
        name: &str,
    ) -> Result<Option<Repository>>;
}
