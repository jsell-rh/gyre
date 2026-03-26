use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Valid function keys for LLM-powered features.
pub const LLM_FUNCTION_KEYS: &[&str] = &[
    "graph-predict",
    "briefing-ask",
    "specs-assist",
    "explorer-generate",
];

/// A customizable prompt template for an LLM-powered function.
///
/// Scoping rules:
/// - workspace_id = Some(_): workspace-specific override
/// - workspace_id = None:    tenant-level default
///
/// Resolution order (most specific wins):
///   workspace override → tenant default → hardcoded PROMPT_DEFAULTS constant
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: Id,
    /// None = tenant-level default (set by tenant admin)
    pub workspace_id: Option<Id>,
    /// Identifies the LLM function. One of:
    ///   "graph-predict" | "briefing-ask" | "specs-assist" | "explorer-generate"
    pub function_key: String,
    /// The prompt content. Variables use `{{var_name}}` syntax.
    pub content: String,
    pub created_by: Id,
    pub created_at: u64,
    pub updated_at: u64,
}
