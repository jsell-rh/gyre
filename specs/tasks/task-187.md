---
title: "Conversational Exploration — AI Q&A grounded in knowledge graph"
spec_ref: "system-explorer.md §8"
depends_on: [task-171, task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §8. Conversational Exploration"
commits: ["7737e532ffab9166fa2a2310dc472f2de39289df"]
---

## Spec Excerpt

system-explorer.md §8 defines Conversational Exploration — an AI-assisted mode where the human asks questions and the system answers from the knowledge graph:

**Example interaction:**
> Q: "Why does SearchService implement both FullTextPort and CachePort?"
> A: "SearchService was initially created to implement search.md (commit abc, agent worker-7). CachePort was added during performance-optimization.md reconciliation (commit def, agent worker-12)..."

The answer is **grounded in the knowledge graph**: provenance records, spec linkage, architectural timeline. The LLM synthesizes, but every claim is traceable to an artifact.

**Follow-up capabilities:**
- "Show me other services that also implement CachePort" → canvas highlights matching nodes
- "What would happen if I removed CachePort from SearchService?" → structural prediction shows impact (ghost overlays from §3)

**Grounding requirements:** Every claim in the LLM's response must reference a concrete artifact (commit SHA, spec path, node ID, agent name). The UI should render these as clickable links that navigate to the referenced entity.

**Data flow:** The conversational Q&A uses the Ask input in the Explorer control bar. Questions are sent to an LLM with knowledge graph context. Responses can include both text answers AND canvas updates (highlight specific nodes, generate a view spec).

## Implementation Plan

1. **Conversational Q&A panel** (`ConversationalExplorer.svelte`):
   - Expandable panel triggered from the Ask input in the control bar
   - Chat-style interface: user question → system response
   - Conversation history maintained in component state (not persisted)
   - Each response can include: text answer, entity references, canvas actions

2. **Backend: Q&A endpoint integration**:
   - Uses the existing Ask input mechanism from task-178
   - For questions that need knowledge graph grounding, the LLM prompt includes:
     - Current graph summary (nodes, edges, types)
     - Provenance data for mentioned entities
     - Spec linkage for mentioned entities
     - Architectural timeline events
   - Response format: text with embedded entity references `[entity:type:id]`

3. **Entity reference rendering**:
   - Parse entity references from LLM response text
   - Render as clickable inline links (styled distinctly from regular text)
   - Click → navigates to the entity in the detail panel or highlights on canvas
   - Types: commit (link to git), spec (open inline), node (highlight on canvas), agent (show provenance)

4. **Canvas integration**:
   - "Show me X" type responses → generate a view spec and apply to canvas
   - "What would happen if..." → trigger structural prediction (ghost overlays from task-186)
   - Highlighting: when response references specific nodes, those nodes pulse on the canvas
   - The canvas responds to conversation context

5. **Follow-up context**:
   - Each follow-up question includes the conversation history
   - The LLM can reference previous answers and build on them
   - "Show me other services" knows which service was discussed

6. **Tests**:
   - Q&A panel renders questions and responses
   - Entity references in responses are clickable
   - Canvas highlights nodes mentioned in responses
   - Follow-up questions maintain conversation context
   - View spec generation from conversational queries works

## Acceptance Criteria

- [ ] Conversational Q&A panel accessible from Ask input in control bar
- [ ] Questions sent with knowledge graph context to LLM
- [ ] Responses grounded in knowledge graph with traceable entity references
- [ ] Entity references rendered as clickable links
- [ ] Click entity reference → navigate to entity or highlight on canvas
- [ ] Follow-up questions maintain conversation context
- [ ] "Show me X" responses update the canvas view
- [ ] "What would happen if..." responses trigger structural predictions
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §8 "Conversational Exploration" for the full specification. The Ask input in the control bar is built in task-178 and connects to `POST /workspaces/:id/explorer-views/generate`. For conversational exploration, the response may be a text answer rather than a view spec — check how the generate endpoint handles questions that cannot produce a view spec (the spec says `{view_spec: null, explanation: "..."}` pattern). The knowledge graph context should include provenance data — check `graph.rs` for how nodes include `spec_path`, `created_by`, `last_modified_by` fields. For structural prediction integration, use the predict endpoint from task-186. The LLM prompt template at `specs/prompts/explorer-generate.md` may need to be extended to support Q&A-style questions alongside view generation.

## Shipped

Conversational exploration (system-explorer.md §8) as a real question →
knowledge-graph → grounded-answer pipeline, in commit 7737e532:

- **Backend** — new `POST /api/v1/workspaces/:id/explorer-views/ask`
  (`ask_explorer` in `api/explorer_views.rs`): the LLM prompt is grounded
  in the workspace's live graph — nodes with spec linkage and commit/agent
  provenance (`created_sha`, `last_modified_sha`, `last_modified_by`,
  recency-ranked, capped), edge summaries, and recent architectural
  timeline deltas — via the new `explorer-ask` prompt template
  (`specs/prompts/explorer-ask.md` + `PROMPT_EXPLORER_ASK` fallback).
  **Grounding enforcement**: the server parses every
  `[entity:node|spec|commit|agent:id]` citation from the answer and
  validates it against the same graph the prompt was built from
  (`validate_entity_refs`); hallucinated ids are stripped to plain text
  and excluded from the returned `entity_refs` (which carry resolved
  labels — node name, spec path, 7-char SHA, agent id). Follow-up turns
  replay client-owned history (max 20, rejected beyond, roles validated).
  Rate limit, budget/cost recording, and template-SHA audit match the
  other LLM endpoints; ABAC maps the route to `explorer_view`/`generate`
  (Developer+ workspace role), registered before `/:view_id`.
- **Canvas actions in the answer** — "show me X" questions return a
  `view_spec` validated through the same chain as the generate endpoint
  (grammar, repo ownership, flow-trace backing with an in-answer fallback
  note); "what would happen if…" questions set `predict`, and the server
  runs the structural prediction through `run_structural_prediction` —
  the core extracted from `POST /repos/:id/graph/predict` so both
  surfaces share one prediction machinery — returning
  `{repo_id, predictions}`.
- **Frontend** — `ConversationalExplorer.svelte`, the third Explorer chat
  mode ("Ask the graph"), also reachable from the control bar's view
  selector ("Ask the graph…" entry). Answers render with clickable,
  distinctly-styled entity references: node → detail panel + canvas
  camera focus, spec → inline spec editor, commit → git remote commit
  page (or copied SHA), agent → highlight the nodes it last modified.
  Node references in a response highlight those nodes on the canvas
  (concept-scope query, search-highlight visual language); view specs
  apply via the existing generated-view path; predictions render as
  ghost overlays through the shared `predictionOverlays` mapping now used
  by both the spec-edit prediction flow and the conversational flow.
  Conversation history lives in component state only (not persisted).

### Test evidence

- `web/src/__tests__/ConversationalExplorer.test.js` (vitest, 6 tests,
  passing here): Q&A rendering, per-type entity-ref click dispatch,
  canvas highlight + view-spec application, prediction application,
  follow-up history replay, error surfacing.
- Server tests in `api/explorer_views.rs` (compile-verified here via
  `cargo check -p gyre-server --lib --tests`; **execution belongs to the
  independent verifier** — the 4 GiB sandbox wrapper blocks gyre-server
  test builds): grounding/citation validation (valid tokens kept,
  hallucinated stripped, labels resolved), view-spec passthrough vs.
  invalid-spec drop, prediction trigger against the single-repo scope,
  prompt grounding (provenance reaches the LLM fully substituted),
  history caps/role rejection, LLM-unavailable 503, plus unit tests for
  `validate_entity_refs` and `short_sha` (char-boundary safe).
- Regression: ExplorerView/ExplorerDefaultViews/GeneratedViewChat
  vitest suites pass (ExplorerDefaultViews expected view-selector list
  extended with the new "Ask the graph…" entry); existing
  `predict_graph` behavior preserved by the extraction (same prompt
  construction, note slot, and response shape).
- Mechanical checks pass: check-arch, check-abac-route-registry,
  check-inert-enforcement (it caught one real flaw mid-development — a
  discarded `enforce_flow_trace_backing` result — fixed by binding it and
  appending the fallback note to the answer), check-abac-exempt-handlers,
  check-mcp-write-tools, check-byte-slice-truncation,
  check-relative-path-defaults, check-lossy-secret-conversion,
  check-unbounded-external-http.

### Verifier must run

    cargo test -p gyre-server --lib api::explorer_views
    cargo test -p gyre-server --lib api::graph::tests
    cd web && npx vitest run src/__tests__/ConversationalExplorer.test.js
