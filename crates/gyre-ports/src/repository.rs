use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Repository;

#[async_trait]
pub trait RepoRepository: Send + Sync {
    async fn create(&self, repo: &Repository) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>>;
    async fn list(&self) -> Result<Vec<Repository>>;
    async fn list_by_project(&self, project_id: &Id) -> Result<Vec<Repository>>;
    async fn update(&self, repo: &Repository) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Repository>>;
}
