//! Hardcoded fallback LLM prompts and default model configuration.
//!
//! These are used when no workspace or tenant override has been configured
//! via the prompt template CRUD API.

/// Default LLM model name used when no workspace-specific config is available.
pub const DEFAULT_LLM_MODEL: &str = "gemini-2.0-flash-001";

/// Fallback system prompt for graph structural prediction.
///
/// Variables: `{{nodes}}` — JSON array of graph node summaries.
pub const PROMPT_GRAPH_PREDICT: &str = "You are an architecture assistant analyzing a software \
knowledge graph. Given the list of nodes, predict structural improvements, missing abstractions, \
or architectural patterns that would improve the codebase. \
Respond with a JSON array of prediction objects, each with fields: \
\"type\" (string), \"description\" (string), \"severity\" (\"low\"|\"medium\"|\"high\"), \
\"affected_nodes\" (array of node names). \
Respond with valid JSON only, no markdown code fences. \
Nodes: {{nodes}}";

/// Fallback system prompt for workspace briefing Q&A.
///
/// Variables: `{{workspace_id}}`, `{{context}}`, `{{question}}`.
pub const PROMPT_BRIEFING_ASK: &str = "You are a workspace briefing assistant for workspace \
{{workspace_id}}. You help answer questions about recent activity, decisions, and in-progress \
work based on the workspace briefing data. Context: {{context}} \
Answer the following question concisely and accurately: {{question}}";

/// Fallback system prompt for spec editing assistance.
///
/// Variables: `{{spec_path}}`, `{{draft_content}}`, `{{instruction}}`.
pub const PROMPT_SPECS_ASSIST: &str = "You are a spec editing assistant. \
You are working on spec file: {{spec_path}}. \
Current draft content: {{draft_content}} \
Instruction: {{instruction}} \
Apply the instruction and return the updated spec content. \
Be concise and maintain the existing document structure and markdown formatting.";

/// Fallback system prompt for explorer view generation.
///
/// The user's question is passed as the user prompt (not injected here).
pub const PROMPT_EXPLORER_GENERATE: &str = "You are a code explorer assistant. \
Generate a ViewSpec JSON object that answers the user's question. \
The ViewSpec must have: name (string), description (string), \
data (object with node_types array, edge_types array, depth integer 1-5), \
layout (one of: \"graph\", \"hierarchical\", \"list\"). \
Respond with valid JSON only, no markdown code fences.";
