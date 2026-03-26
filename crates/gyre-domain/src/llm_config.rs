use serde::{Deserialize, Serialize};

/// Per-function, per-workspace LLM model configuration.
///
/// Resolution order when an LLM call is made:
///   1. Workspace-level override (workspace_id = Some(...))
///   2. Tenant-level default (workspace_id = None)
///   3. GYRE_LLM_MODEL env var
///   4. Hardcoded default ("gemini-2.0-flash-001")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmFunctionConfig {
    pub id: gyre_common::Id,
    /// None = tenant-level default
    pub workspace_id: Option<gyre_common::Id>,
    /// One of: "graph-predict" | "briefing-ask" | "specs-assist" | "explorer-generate"
    pub function_key: String,
    /// Vertex AI model name, e.g. "gemini-2.0-flash-001", "gemini-1.5-pro-002"
    pub model_name: String,
    /// Optional max_tokens cap for this function (None = provider default)
    pub max_tokens: Option<u32>,
    pub updated_by: gyre_common::Id,
    pub updated_at: u64,
}

/// Valid function keys for LLM configuration.
pub const VALID_FUNCTION_KEYS: &[&str] = &[
    "graph-predict",
    "briefing-ask",
    "specs-assist",
    "explorer-generate",
];

/// Returns true if the given function key is valid.
pub fn is_valid_function_key(key: &str) -> bool {
    VALID_FUNCTION_KEYS.contains(&key)
}
