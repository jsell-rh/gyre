---
title: "HSI Explorer Generated Views (LLM-Powered)"
spec_ref: "human-system-interface.md §3 Generated Views"
depends_on:
  - task-088
  - task-070
progress: not-started
coverage_sections:
  - "human-system-interface.md §3 Generated Views (LLM-Powered, On-Demand)"
commits: []
---

## Spec Excerpt

The user asks a question in natural language. The LLM translates it to a graph query + layout, producing a focused view.

```
User: "How does authentication work?"

LLM generates view definition:
{
  "name": "How auth works",
  "data": {
    "concept": "auth",
    "node_types": ["Module", "Function", "Type", "Endpoint"],
    "depth": 2
  },
  "layout": "hierarchical",
  "highlight": {"spec_path": "specs/system/identity-security.md"},
  "explanation": "Authentication flows through require_auth_middleware..."
}
```

The view renders with the LLM's explanation as a sidebar annotation. The user can save the view for reuse or refine the question.

**Flow layout:** The LLM selects `"flow"` layout when the question implies data movement ("how does X flow", "what happens when Y"). For structural questions ("what is X made of"), it uses `"graph"` or `"hierarchical"`.

**MR ID discovery for `trace_source`:** The LLM prompt template injects recent MRs with trace data. If no matching MR has trace data, the LLM responds with `"graph"` layout.

**Important constraint:** The LLM has access only to the knowledge graph API (read-only). It cannot modify code, create tasks, or trigger actions. It is a *query translator*, not an agent.

## Implementation Plan

1. **LLM view generation endpoint:**
   - An endpoint for generating views from natural language already exists (check `explorer-views/generate` or similar in the Explorer chat)
   - Verify the LLM system prompt includes the view spec grammar (data + layout + encoding + annotations + explanation)
   - Ensure the LLM output is validated against the view spec grammar before rendering

2. **View spec grammar compliance:**
   - Generated views must use the same schema as saved views: `data`, `layout`, `encoding`, optional `annotations`, `explanation`
   - Layout options: `"graph"`, `"hierarchical"`, `"list"`, `"flow"`
   - The LLM selects layout based on question intent

3. **Flow layout for trace-backed views:**
   - When the LLM generates a `"flow"` layout with `trace_source`, fetch trace data from `GET /api/v1/merge-requests/:id/trace`
   - Render animated particles following the span tree
   - If no trace data available, fall back to `"graph"` layout with explanation

4. **MR discovery for trace questions:**
   - Inject into LLM prompt: list of recent MRs with trace data (`GET /workspaces/:id/merge-requests?has_trace=true&limit=10`)
   - LLM matches question to relevant MR by spec_ref or title

5. **Explanation sidebar:**
   - When a generated view has an `explanation` field, render it as a sidebar annotation alongside the graph
   - The explanation is LLM-generated prose, read-only

6. **Save generated view:**
   - "Save this view" button on generated views
   - Saves via `POST /api/v1/workspaces/:workspace_id/explorer-views`
   - Saved view preserves the generated query + layout + explanation

7. **Refine question:**
   - Follow-up input: "Show me just the middleware chain" refines the current view
   - The LLM receives the previous view definition as context for refinement

## Acceptance Criteria

- [ ] Natural language question produces a valid view definition (graph query + layout)
- [ ] Generated views render on the ExplorerCanvas
- [ ] LLM selects `"flow"` layout for data movement questions
- [ ] LLM selects `"graph"` or `"hierarchical"` for structural questions
- [ ] Explanation appears as sidebar annotation
- [ ] Generated view can be saved to workspace saved views
- [ ] Follow-up questions refine the current view
- [ ] If no trace data, flow questions fall back to graph layout
- [ ] LLM has read-only access only (no modifications)
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Generated Views (LLM-Powered, On-Demand)" for the full spec. This depends on task-088 (default views give the canvas rendering foundation) and task-070 (Explorer LLM agent). Check the existing ExplorerChat component in `web/src/lib/ExplorerChat.svelte` for the chat interface. The view generation endpoint may already exist — look for `explorer-views/generate` in the API routes. The view spec grammar is defined in `specs/system/view-query-grammar.md` (tasks 062-064). The LLM should output JSON conforming to this grammar. For flow layout with trace data, the trace endpoint is `GET /api/v1/merge-requests/:id/trace` (already registered).
