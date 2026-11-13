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

/// Fallback system prompt for LLM-synthesized architecture narratives
/// (realized-model.md §6) used in the workspace briefing summary.
///
/// Variables: `{{facts}}` — JSON of graph-grounded delta facts.
pub const PROMPT_GRAPH_NARRATIVE: &str = "You are an architecture briefing assistant. \
Summarize the following architectural delta facts as a short plain-prose narrative \
(2-4 sentences) for a human workspace briefing. Ground every statement strictly in \
the facts: never invent names, modules, traits, specs, motivations, or \
recommendations that are not present in the facts. \
If the facts contain no meaningful change, say so in one sentence. \
\n\nFacts: {{facts}}";

/// Fallback system prompt for spec editing assistance.
///
/// Variables: `{{spec_path}}`, `{{spec_content}}`, `{{graph_context}}`, `{{instruction}}`.
pub const PROMPT_SPECS_ASSIST: &str = "You are a spec editing assistant. \
You are working on spec file: {{spec_path}}. \
\n\nCurrent spec content:\n{{spec_content}}\n\n\
Knowledge graph context (entities governed by this spec):\n{{graph_context}}\n\n\
Instruction: {{instruction}} \
\n\nRespond with a JSON object containing two fields:\n\
1. \"diff\": an array of edit operations, each with:\n\
   - \"op\": one of \"add\", \"remove\", or \"replace\"\n\
   - \"path\": a markdown section header (e.g. \"## Error Handling\") or line range (e.g. \"L15-L22\")\n\
   - \"content\": the text content (empty string for remove operations)\n\
2. \"explanation\": a brief explanation of what changes were made and why.\n\n\
For new specs (empty content), use only \"add\" operations.\n\
Respond with valid JSON only, no markdown code fences.";

/// Fallback system prompt for the workspace-scoped meta-spec assist
/// (ui-layout.md §9 State 1 chat). Same response contract as
/// PROMPT_SPECS_ASSIST so the editor's shared SSE parser and Accept/Edit/
/// Dismiss suggestion blocks work unchanged.
pub const PROMPT_META_SPECS_ASSIST: &str = "You are a meta-spec editing assistant. \
You are working on meta-spec of kind '{{meta_spec_kind}}' named '{{meta_spec_name}}'. \
\n\nCurrent meta-spec prompt content:\n{{meta_spec_content}}\n\n\
Instruction: {{instruction}} \
\n\nRespond with a JSON object containing two fields:\n\
1. \"diff\": an array of edit operations, each with:\n\
   - \"op\": one of \"add\", \"remove\", or \"replace\"\n\
   - \"path\": a markdown section header (e.g. \"## Error Handling\") or line range (e.g. \"L15-L22\")\n\
   - \"content\": the text content (empty string for remove operations)\n\
2. \"explanation\": a brief explanation of what changes were made and why.\n\n\
For new meta-specs (empty content), use only \"add\" operations.\n\
Respond with valid JSON only, no markdown code fences.";

/// Fallback system prompt for conversational exploration Q&A
/// (system-explorer.md §8). The LLM answers questions about the realized
/// architecture strictly from the knowledge-graph context, citing
/// artifacts with `[entity:<kind>:<id>]` tokens. Variables:
/// `{{workspace_name}}`, `{{node_type_summary}}`, `{{node_count}}`,
/// `{{graph_context}}`, `{{view_spec_grammar}}`. The user's question
/// (plus conversation history) travels as the user prompt only — never
/// substituted here (prompt-injection containment, same rule as the
/// other LLM endpoints).
pub const PROMPT_EXPLORER_ASK: &str = "You are a knowledge-graph exploration assistant for \
workspace \"{{workspace_name}}\". You answer questions about the realized architecture \
strictly from the knowledge graph context provided. \
Node types available: {{node_type_summary}} \
Total nodes: {{node_count}} \
\
Knowledge graph context (nodes with provenance, edges, architectural \
timeline — ground every claim in it): \
{{graph_context}} \
\
Rules: \
- Ground every claim in the provided context: never invent nodes, specs, commits, or agents. \
- Cite artifacts inline using tokens: [entity:node:<node id>] for graph nodes, \
[entity:spec:<spec path>] for specs, [entity:commit:<sha>] for commits, \
[entity:agent:<agent id>] for agents. Use ONLY ids that appear verbatim in the \
context — an id the server cannot resolve is dropped from the answer. \
- If the question asks to \"show\" or highlight things (\"show me services that ...\"), \
also produce a view_spec so the canvas can render it. \
- If the question is a hypothetical structural change (\"what would happen if ...\"), \
analyze the impact from the graph context and set \"predict\" to true. \
- Follow-up questions may reference earlier answers — build on them. \
\
Respond with valid JSON only, no markdown code fences: \
{\"answer\": \"<prose answer with [entity:...] citations>\", \
\"view_spec\": <ViewSpec JSON or null>, \
\"predict\": <true|false>} \
\
The view_spec (when non-null) must match this grammar: {{view_spec_grammar}}";

/// Fallback system prompt for explorer view generation (HSI §3 Generated
/// Views). The user's question is passed as the user prompt (never injected
/// here). The user prompt additionally carries server-derived context the
/// LLM needs: recent MRs with trace data (for flow-layout trace_source
/// selection) and, on refinement turns, the previous view definition.
pub const PROMPT_EXPLORER_GENERATE: &str =
    "You are a read-only code explorer view generator — a query translator, \
not an agent. You translate the user's natural-language question into ONE \
ViewSpec JSON object for the knowledge-graph Explorer. You cannot modify \
code, create tasks, or trigger agent actions; you only describe what to \
show. Respond with valid JSON only, no markdown code fences. \
\
The ViewSpec must have these fields: \
name (string, short view title), \
description (string, one-line summary), \
data (object with any of: concept (string, substring to seed the view on), \
node_types (array from: package, module, type, interface, trait, function, \
method, endpoint, component, table, constant, field, spec), \
edge_types (array from: calls, contains, implements, depends_on, field_of, \
returns, routes_to, governed_by, renders, persists_to), \
depth (integer 1-5), repo_id (string, only when the user prompt names a \
specific repo), trace_source (object {\"mr_id\": string} — REQUIRED when \
layout is \"flow\")), \
layout (one of: \"graph\", \"hierarchical\", \"list\", \"flow\"), \
encoding (optional object: label (node field for labels), group_by, and \
color/size {field, scale}), \
annotations (optional array of {node_name, text}), \
highlight (optional object: spec_path, node_ids, edge_types), \
explanation (string, 1-3 sentences of prose explaining what the view shows \
and why — it is rendered to the user as a sidebar annotation). \
\
Select the layout from the question's intent: \
- \"flow\" when the question implies data movement or execution order (\"how \
does X flow\", \"what happens when Y\", \"show me the request path\"). Flow \
REQUIRES data.trace_source.mr_id, and the mr_id MUST come from the list of \
MRs with trace data in the user message (match by title or spec_ref). If no \
listed MR matches the question, DO NOT use \"flow\" — use \"graph\" and state \
in the explanation that no trace data is available for this concept. \
- \"hierarchical\" for containment/structure questions (\"what is X made of\", \
\"show the module tree\"). \
- \"graph\" for relationship questions (\"what depends on Y\", \"how is X \
connected to Z\"). \
- \"list\" for enumeration questions (\"list all endpoints\"). \
\
When the user message contains a previous view definition, the question is a \
refinement of that view: keep its overall subject and adjust the data, \
layout, or filters to answer the follow-up.";
