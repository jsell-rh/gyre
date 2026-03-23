use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::SpecApproval;

#[async_trait]
pub trait SpecApprovalRepository: Send + Sync {
    async fn create(&self, approval: &SpecApproval) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<SpecApproval>>;
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>>;
    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>>;
    async fn list_all(&self) -> Result<Vec<SpecApproval>>;
    async fn revoke(&self, id: &Id, revoked_by: &str, reason: &str, now: u64) -> Result<()>;
    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<()>;
}
