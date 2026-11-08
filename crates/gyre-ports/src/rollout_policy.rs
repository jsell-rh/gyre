use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;

/// Repository for per-workspace rollout policies (meta-spec-reconciliation.md §7).
///
/// Same persistence model as `MetaSpecSetRepository`: the policy is stored as
/// an opaque JSON string keyed by workspace ID; deserialization to
/// `gyre_domain::RolloutPolicy` happens in the server layer. A missing row
/// means "use the spec default policy" — it is not an error.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait RolloutPolicyRepository: Send + Sync {
    /// Get the raw JSON for a workspace's rollout policy, or `None` if unset.
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>>;

    /// Upsert (insert or replace) the rollout policy for a workspace.
    async fn upsert(&self, workspace_id: &Id, json: &str) -> Result<()>;

    /// Delete the rollout policy for a workspace (reverts to the default).
    async fn delete(&self, workspace_id: &Id) -> Result<()>;
}
