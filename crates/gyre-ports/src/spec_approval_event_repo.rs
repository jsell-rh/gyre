//! Port trait for spec approval event history persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::SpecApprovalEvent;

#[async_trait]
pub trait SpecApprovalEventRepository: Send + Sync {
    async fn record(&self, event: &SpecApprovalEvent) -> Result<()>;
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApprovalEvent>>;
    async fn list_all(&self) -> Result<Vec<SpecApprovalEvent>>;
    async fn revoke_event(
        &self,
        id: &str,
        revoked_at: u64,
        revoked_by: &str,
        reason: &str,
    ) -> Result<()>;
}
