use gyre_common::Id;

use crate::AppState;

/// Default LLM model used when no config is set and no env override is present.
pub const DEFAULT_LLM_MODEL: &str = "gemini-2.0-flash-001";

/// Resolve the model name and max_tokens for an LLM function in a workspace.
///
/// Resolution order:
///   1. Workspace config override (workspace_id + function_key)
///   2. Tenant default (function_key, workspace_id = None)
///   3. `GYRE_LLM_MODEL` environment variable
///   4. Hardcoded default ("gemini-2.0-flash-001")
pub async fn resolve_llm_model(
    state: &AppState,
    workspace_id: &Id,
    function_key: &str,
) -> (String, Option<u32>) {
    resolve_llm_model_with_default(state, workspace_id, function_key, DEFAULT_LLM_MODEL).await
}

/// Same resolution order as [`resolve_llm_model`], but the level-4 hardcoded
/// default is caller-supplied. Used by surfaces whose spec mandates a different
/// default model than the platform-wide one (e.g. the explorer's Claude Agent
/// SDK path defaults to `claude-sonnet-4-6` per explorer-implementation.md §8;
/// the native Vertex-backed fallback keeps the gemini default because claude
/// model names are not valid there).
pub async fn resolve_llm_model_with_default(
    state: &AppState,
    workspace_id: &Id,
    function_key: &str,
    fallback_default: &str,
) -> (String, Option<u32>) {
    if let Ok(Some(cfg)) = state
        .llm_configs
        .get_effective(workspace_id, function_key)
        .await
    {
        return (cfg.model_name, cfg.max_tokens);
    }
    let model = std::env::var("GYRE_LLM_MODEL").unwrap_or_else(|_| fallback_default.to_string());
    (model, None)
}
