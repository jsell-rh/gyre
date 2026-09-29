use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Persona, PersonaScope, Policy, Workspace};

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

    /// Atomically apply a workspace trust transition in a single DB transaction.
    ///
    /// Upserts the workspace row, optionally deletes every workspace-scoped
    /// `trust:`-prefixed policy (`delete_trust_policies = true`), then creates
    /// each policy in `new_policies`. On any failure the whole transaction rolls
    /// back — the workspace and its policies are never left partially written.
    ///
    /// Used for both initial creation (seed policies, `delete_trust_policies =
    /// false`) and trust-level updates (`delete_trust_policies = true` for every
    /// non-`Custom` transition; `Custom` preserves existing `trust:` policies).
    async fn apply_trust_transition(
        &self,
        workspace: &Workspace,
        delete_trust_policies: bool,
        new_policies: &[Policy],
    ) -> Result<()>;
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
