//! Port trait for container audit record persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::ContainerAuditRecord;

#[async_trait]
pub trait ContainerAuditRepository: Send + Sync {
    async fn find_by_agent_id(&self, agent_id: &str) -> Result<Option<ContainerAuditRecord>>;
    async fn save(&self, record: &ContainerAuditRecord) -> Result<()>;
    async fn update_exit(
        &self,
        agent_id: &str,
        exit_code: Option<i32>,
        stopped_at: Option<u64>,
    ) -> Result<()>;
}
