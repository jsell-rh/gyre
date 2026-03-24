//! Port trait for per-repo spec enforcement policy persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::SpecPolicy;

#[async_trait]
pub trait SpecPolicyRepository: Send + Sync {
    async fn get_for_repo(&self, repo_id: &str) -> Result<SpecPolicy>;
    async fn set_for_repo(&self, repo_id: &str, policy: SpecPolicy) -> Result<()>;
}
