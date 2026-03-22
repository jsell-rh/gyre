use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{WorkspaceMembership, WorkspaceRole};

#[async_trait]
pub trait WorkspaceMembershipRepository: Send + Sync {
    async fn create(&self, membership: &WorkspaceMembership) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<WorkspaceMembership>>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<WorkspaceMembership>>;
    async fn list_by_user(&self, user_id: &Id) -> Result<Vec<WorkspaceMembership>>;
    async fn find_by_user_and_workspace(
        &self,
        user_id: &Id,
        workspace_id: &Id,
    ) -> Result<Option<WorkspaceMembership>>;
    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()>;
    async fn accept(&self, id: &Id, now: u64) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
