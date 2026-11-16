use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Tenant;

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait TenantRepository: Send + Sync {
    /// Fails if a tenant with the same `id`, or the same `slug`, already
    /// exists (slug is the tenant's unique handle; SQLite enforces this via
    /// the `tenants.slug UNIQUE` constraint — every adapter must match it).
    async fn create(&self, tenant: &Tenant) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Tenant>>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>>;
    async fn list(&self) -> Result<Vec<Tenant>>;
    async fn update(&self, tenant: &Tenant) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
