//! Port trait for per-workspace dependency enforcement policy persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::DependencyPolicy;

/// Port for persisting per-workspace dependency enforcement policies.
#[async_trait]
pub trait DependencyPolicyRepository: Send + Sync {
    /// Get the dependency policy for a workspace. Returns the default if none configured.
    async fn get_for_workspace(&self, workspace_id: &Id) -> Result<DependencyPolicy>;

    /// Set the dependency policy for a workspace.
    async fn set_for_workspace(&self, workspace_id: &Id, policy: &DependencyPolicy) -> Result<()>;
}
