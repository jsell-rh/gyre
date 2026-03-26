use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::PromptTemplate;

#[async_trait]
pub trait PromptRepository: Send + Sync {
    /// Resolve effective prompt: workspace override → tenant default → None.
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<PromptTemplate>>;

    /// List workspace-scoped overrides (not inherited defaults).
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<PromptTemplate>>;

    /// Upsert a workspace-scoped override.
    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<PromptTemplate>;

    /// Upsert a tenant-level default (workspace_id = None).
    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<PromptTemplate>;

    /// Delete workspace override — reverts to tenant/hardcoded default.
    async fn delete_workspace_override(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<()>;
}
