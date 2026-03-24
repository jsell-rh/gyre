//! Port trait for merge attestation bundle persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::AttestationBundle;

#[async_trait]
pub trait AttestationRepository: Send + Sync {
    async fn find_by_mr_id(&self, mr_id: &str) -> Result<Option<AttestationBundle>>;
    async fn save(&self, mr_id: &str, bundle: &AttestationBundle) -> Result<()>;
}
