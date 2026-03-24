//! Port trait for per-repo push gate configuration persistence.

use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait PushGateRepository: Send + Sync {
    async fn get_for_repo(&self, repo_id: &str) -> Result<Vec<String>>;
    async fn set_for_repo(&self, repo_id: &str, gates: Vec<String>) -> Result<()>;
}
