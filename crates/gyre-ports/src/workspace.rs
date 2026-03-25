use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Persona, PersonaScope, Workspace};

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    async fn create(&self, workspace: &Workspace) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Workspace>>;
    /// Find a workspace by slug within a specific tenant.
    /// Required for git URL resolution: /git/:workspace_slug/:repo_name/*.
    async fn find_by_slug(&self, tenant_id: &Id, slug: &str) -> Result<Option<Workspace>>;
    async fn list(&self) -> Result<Vec<Workspace>>;
    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<Workspace>>;
    async fn update(&self, workspace: &Workspace) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait PersonaRepository: Send + Sync {
    async fn create(&self, persona: &Persona) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Persona>>;
    async fn find_by_slug_and_scope(
        &self,
        slug: &str,
        scope: &PersonaScope,
    ) -> Result<Option<Persona>>;
    async fn list(&self) -> Result<Vec<Persona>>;
    async fn list_by_scope(&self, scope: &PersonaScope) -> Result<Vec<Persona>>;
    async fn update(&self, persona: &Persona) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
