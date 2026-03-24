use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Tenant;

#[async_trait]
pub trait TenantRepository: Send + Sync {
    async fn create(&self, tenant: &Tenant) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Tenant>>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>>;
    async fn list(&self) -> Result<Vec<Tenant>>;
    async fn update(&self, tenant: &Tenant) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
