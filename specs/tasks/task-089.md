---
title: "HSI Explorer Generated Views (LLM-Powered)"
spec_ref: "human-system-interface.md §3 Generated Views"
depends_on:
  - task-088
  - task-070
progress: complete
coverage_sections:
  - "human-system-interface.md §3 Generated Views (LLM-Powered, On-Demand)"
commits: ["74af4c305c0db43c839a7e4eb262ec9826537048", "801a872a300d95fd93eaae6db572a5d6b04b4336", "a72026204dd00e01ac101f21094b5b7e760a3b57"]
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

## Shipped

The generated-views flow is now real end-to-end on the workspace `explorer-views/generate` surface (the endpoint existed but its prompt was a three-line stub, its explanation was a hardcoded string, MR discovery / flow fallback / refinement did not exist, and no UI ever called it — `api.generateExplorerView` had zero callers).

### Backend (`crates/gyre-server`)

- **Prompt (`llm_defaults.rs`)**: `PROMPT_EXPLORER_GENERATE` now carries the full ViewSpec grammar (name, description, data{concept, node_types, edge_types, depth, repo_id, trace_source}, layout, encoding, annotations, highlight, explanation) and the layout-selection rules from the spec: `"flow"` for data-movement questions (requires `trace_source.mr_id` chosen from the injected MR list), `"hierarchical"` for containment, `"graph"` for relationships, `"list"` for enumerations; plus the read-only query-translator constraint and refinement-turn instructions. User input still never enters the system prompt (strip + user-prompt pattern preserved).
- **MR ID discovery (`explorer_views.rs`)**: `recent_mrs_with_traces` lists the workspace's MRs, keeps those with a captured trace (same `traces.get_by_mr` lookup that serves `GET /merge-requests/:id/trace`, so the list cannot drift from the trace endpoint), sorts newest-first, caps at 10, and `build_generate_user_prompt` injects it (mr_id, title, spec_ref, status) ahead of the framed question. Empty list → explicit "none — do not use flow" instruction. `GET /api/v1/workspaces/:id/merge-requests` gained the specced `?has_trace=true&limit=N` query params (shared semantics, tested).
- **Flow fallback (server-enforced)**: `enforce_flow_trace_backing` rewrites a `"flow"` layout to `"graph"` (stripping `trace_source`) unless the referenced MR exists, belongs to this workspace, and currently has a captured trace — hallucinated and cross-workspace mr_ids fall back too. The fallback note is appended to the LLM's explanation.
- **Explanation pass-through**: the `complete` SSE event now carries the LLM's own `explanation` (falling back to `description`, then a generic string) instead of the hardcoded `"Generated view for: …"`.
- **Refinement**: `GenerateViewRequest` gained `previous_spec`, validated against the same grammar + repo-ownership checks (400 on invalid) before the LLM sees it, and injected into the user prompt as the view being refined.

### Frontend (`web/src`)

- **`lib/generated-view.js`**: `viewSpecToViewQuery` (ViewSpec → canvas ViewQuery: concept/filter scopes with type+edge normalization, `$governed_by` computed filters with quote escaping, annotations → callouts, name+explanation → the canvas query annotation) and `parseGenerateSse`/`readGenerateSse` (SSE body parsing).
- **`lib/GeneratedViewChat.svelte`**: the Generated Views chat panel — question input, SSE handling, immediate apply of the generated view, per-view **Save this view** button (`POST /workspaces/:id/explorer-views`, preserving query + layout + explanation), and follow-up questions that send `previous_spec` so they refine the current view. The panel only ever sends a question — the LLM stays a read-only query translator.
- **`components/ExplorerView.svelte`**: the chat column gained mode tabs (*Explorer* WS chat | *Generated Views*); `applyGeneratedViewSpec` applies the translated query to the ExplorerCanvas and, for `"flow"` layouts, fetches the `trace_source` MR's trace and starts playback (client mirror of the server fallback on fetch failure).
- **`lib/ExplorerCanvas.svelte`**: new nonce-based `playbackRequest` prop triggers `startTracePlayback()` (evaluative lens + animated particles along the span tree) for generated flow views.

### Acceptance criteria mapping

- NL question → valid view definition: generate endpoint + full-grammar prompt + existing `parse_and_validate` grammar gate (invalid LLM output still yields `view_spec: null` + fallback view).
- Renders on ExplorerCanvas: `viewSpecToViewQuery` → `activeViewQuery` (integration test asserts the rendered canvas state).
- Flow for data-movement / graph-hierarchical for structural: prompt rules + grammar; flow additionally requires real trace backing (server-enforced).
- Explanation as sidebar annotation: LLM explanation → canvas `.query-annotation` (title = view name, description = explanation) + panel message.
- Save: `saveExplorerView` with name/description/spec (component test pins the exact payload).
- Refine: `previous_spec` on follow-ups (component test pins request shape).
- No-trace fallback: `enforce_flow_trace_backing` (three handler tests: untraced MR, foreign-workspace MR, pass-through with a traced MR).
- Read-only LLM: prompt constraint + the endpoint only calls `predict_json` (no tools, no writes).

### Test evidence

- Rust: 23 tests in `api::explorer_views` (7 new: flow pass-through, flow→graph fallback ×2, invalid previous_spec 400, prompt-injection unit test, explanation fallback chain, MR-discovery ordering/filtering) + `api::merge_requests::tests::list_workspace_mrs_filters_by_trace_and_limits` — all pass (`cargo test -p gyre-server --lib api::explorer_views` / `api::merge_requests`); re-run green after the formatting repair (commit `74af4c305c`).
- Web: `generated-view.test.js` (14 unit tests), `GeneratedViewChat.test.js` (5 component tests), `ExplorerGeneratedViews.test.js` (3 full-tree integration tests: mode tabs, canvas annotation rendering, flow-view trace fetch + evaluative-lens switch). Full `npm test` in `web/`: **86 files, 1883 passed, 41 skipped, 0 failed**.
- `scripts/check-arch.sh` and `scripts/check-abac-route-registry.sh` pass (no new routes; query params only).

### Verification repair (2025 assignment)

The prior verification run failed only on changed-lines rustfmt in `explorer_views.rs`, `merge_requests.rs`, and `llm_defaults.rs` (every other gate in that log passed). Commit `74af4c305c` runs `cargo fmt` on exactly those files — no behavior change; `cargo fmt --all --check` is now clean and the focused Rust tests above were re-run and pass.

web/dist was deliberately not committed — the executor rebuilds and publishes generated assets from the assignment base.
