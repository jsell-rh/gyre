use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{MergeRequest, MrStatus};

#[async_trait]
pub trait MergeRequestRepository: Send + Sync {
    async fn create(&self, mr: &MergeRequest) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>>;
    async fn list(&self) -> Result<Vec<MergeRequest>>;
    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>>;
    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>>;
    async fn update(&self, mr: &MergeRequest) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
