use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::ComputeTargetEntity;

#[async_trait]
pub trait ComputeTargetRepository: Send + Sync {
    /// Persist a new compute target.
    async fn create(&self, target: &ComputeTargetEntity) -> Result<()>;

    /// Fetch a compute target by its ID.
    async fn get_by_id(&self, id: &Id) -> Result<Option<ComputeTargetEntity>>;

    /// List all compute targets for a tenant.
    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<ComputeTargetEntity>>;

    /// Persist updates to an existing compute target.
    async fn update(&self, target: &ComputeTargetEntity) -> Result<()>;

    /// Delete a compute target.
    /// Callers must check for workspace references before calling this.
    async fn delete(&self, id: &Id) -> Result<()>;

    /// Return the current default compute target for a tenant, if any.
    async fn get_default_for_tenant(&self, tenant_id: &Id) -> Result<Option<ComputeTargetEntity>>;

    /// Check whether any workspace references the given compute target.
    async fn has_workspace_references(&self, id: &Id) -> Result<bool>;
}
