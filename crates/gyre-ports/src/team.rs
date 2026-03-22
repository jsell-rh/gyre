use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Team;

#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn create(&self, team: &Team) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Team>>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>>;
    async fn update(&self, team: &Team) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
