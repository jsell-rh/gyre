use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Project;

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create(&self, project: &Project) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Project>>;
    async fn list(&self) -> Result<Vec<Project>>;
    async fn update(&self, project: &Project) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Project>>;
}
