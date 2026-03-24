//! Port trait for quality gate persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::{GateResult, GateStatus, QualityGate};

#[async_trait]
pub trait QualityGateRepository: Send + Sync {
    async fn save(&self, gate: &QualityGate) -> Result<()>;
    async fn find_by_id(&self, id: &str) -> Result<Option<QualityGate>>;
    async fn list_by_repo_id(&self, repo_id: &str) -> Result<Vec<QualityGate>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

#[async_trait]
pub trait GateResultRepository: Send + Sync {
    async fn save(&self, result: &GateResult) -> Result<()>;
    async fn update_status(
        &self,
        id: &str,
        status: GateStatus,
        started_at: Option<u64>,
        finished_at: Option<u64>,
        output: Option<String>,
    ) -> Result<()>;
    async fn find_by_id(&self, id: &str) -> Result<Option<GateResult>>;
    async fn list_by_mr_id(&self, mr_id: &str) -> Result<Vec<GateResult>>;
}
