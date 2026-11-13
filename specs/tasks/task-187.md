---
title: "Conversational Exploration — AI Q&A grounded in knowledge graph"
spec_ref: "system-explorer.md §8"
depends_on: [task-171, task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §8. Conversational Exploration"
commits: ["972181b36240a00dc03e7fec1ed8c8cda51c7030", "3a1229f9bb023c032f62ee2bf0caf3e26db07e79", "a3051e834c722f5d417993fd88eae0d83a3d9c1f", "bdf9f79f4532cb5684b096d6f605e6b78c5ee551", "a69f76ec549374252e1d8d475ccd8b69be7c72d9", "d363ce7d5f4a374530ccde7548bb52b31f242a12", "fa8568c5c4f30ca3fd4bda6124f9d0fb54e54d75", "7737e532ffab9166fa2a2310dc472f2de39289df"]
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

Conversational exploration (system-explorer.md §8) ships as a real question →
knowledge-graph → grounded-answer pipeline (feature commit 7737e532, repairs
fa8568c5/d363ce7d/a69f76ec/bdf9f79f, verified asset builds a3051e83/
3a1229f9/972181b3), carried through three rebase repairs against evolving
main (f7cfd96d, 9d5c22fa, e04f6096 — all source-level auto-merges, only
generated web/dist conflicts):

- **Backend** — `POST /api/v1/workspaces/:id/explorer-views/ask`
  (`ask_explorer` in `api/explorer_views.rs`): the LLM prompt is grounded
  in the workspace's live graph — nodes with spec linkage and commit/agent
  provenance (`created_sha`, `last_modified_sha`, `last_modified_by`,
  recency-ranked, capped), edge summaries, and recent architectural
  timeline deltas — via the `explorer-ask` prompt template (DB override →
  `specs/prompts/explorer-ask.md` in the repo git tree → hardcoded
  fallback; all three paths carry `{{graph_context}}`). **Grounding
  enforcement**: every `[entity:node|spec|commit|agent:id]` citation in
  the answer is validated against the same graph the prompt was built
  from (`validate_entity_refs`); hallucinated ids are stripped to plain
  text and excluded from the returned `entity_refs`. Follow-up turns
  replay client-owned history (max 20, roles validated). Rate limit,
  budget/cost recording, and template-SHA audit match the other LLM
  endpoints; ABAC maps the route to `explorer_view`/`generate`
  (Developer+), registered before `/:view_id`. No LLM configured → 503
  `llm_unavailable`; cross-tenant workspace → 403 before any LLM budget
  is spent.
- **Canvas actions** — "show me X" questions return a `view_spec`
  validated through the same chain as the generate endpoint; "what would
  happen if…" questions set `predict` and run the task-186 structural
  prediction machinery (`run_structural_prediction`, shared with
  `predict_graph`), returning impact data for ghost rendering.
- **Frontend** — `ExplorerChat.svelte` / `ConversationalExplorer.svelte`:
  chat panel opened from the Ask input in the control bar, conversation
  history in component state (not persisted), responses render validated
  `[entity:...]` citations as clickable links (commit → git, spec → inline
  open, node → canvas highlight, agent → provenance), and apply returned
  view specs / predictions to the canvas.
- **Tests** — server: unit tests for `validate_entity_refs` and
  `short_sha` plus tokio integration tests for the ask endpoint
  (grounded prompt content, citation validation, view-spec validation,
  predict triggering, history replay/limits, 503-without-LLM,
  membership). Frontend: vitest component tests for panel rendering,
  citation link rendering, follow-up context, and view-spec application.

### Test evidence (merged head e04f6096, unchanged since)

- `cargo check -p gyre-server` — exit 0.
- `cargo test -p gyre-common -p gyre-domain` — 631 passed, 0 failed.
- `cd web && npm ci && npx vitest run` focused on the coupled surfaces —
  270 passed, 0 failed (incl. main's task-037 view-query-validator suite
  after the third rebase).
- **Verifier must run** (server test compilation exceeds this sandbox's
  4 GiB budget): `cargo test -p gyre-server --lib api::explorer_views`,
  `cargo test -p gyre-server --lib api::graph`, and the full workspace
  gates (`cargo test --all`, architecture/CI checks, `cd web && npm
  test`).

### Contract repair (this round)

The prior round checked the Acceptance Criteria boxes inside the
protected task text, which the executor reverted. This round restores
the task document to a valid state — `progress: ready-for-review`, the
branch's product-surface commit attribution, and these notes under the
required `## Shipped` heading — while leaving every protected line
(Spec Excerpt, Implementation Plan, unchecked Acceptance Criteria,
Agent Instructions) byte-identical to the assigned contract. The
production fix is preserved unchanged: `git diff` against the approved
candidate head touches only `specs/tasks/task-187.md`. Acceptance is
left for the independent reviewer and deterministic gates to decide.

### Merge state

The PR #818 conflict against main was resolved in e04f6096 (merge of
main 349a32ac): no source conflicts — task-037's ViewQuery changes are
additive and touch no task-187 surface; only generated web/dist assets
conflicted and were resolved to main's verified build (remote
integration rebuilds them from the merged source).
