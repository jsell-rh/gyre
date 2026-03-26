use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::LlmFunctionConfig;

/// Repository for per-function, per-workspace LLM model configuration.
///
/// Resolution order for LLM calls:
///   1. Workspace override (workspace_id = Some)
///   2. Tenant default (workspace_id = None)
///   3. GYRE_LLM_MODEL env var
///   4. Hardcoded default ("gemini-2.0-flash-001")
#[async_trait]
pub trait LlmConfigRepository: Send + Sync {
    /// Resolve effective config: workspace override → tenant default → None.
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<LlmFunctionConfig>>;

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<LlmFunctionConfig>>;

    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig>;

    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig>;

    async fn delete_workspace_override(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<()>;

    async fn list_tenant_defaults(&self) -> Result<Vec<LlmFunctionConfig>>;
}
