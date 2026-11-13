---
title: "Conversational Exploration — AI Q&A grounded in knowledge graph"
spec_ref: "system-explorer.md §8"
depends_on: [task-171, task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §8. Conversational Exploration"
commits: ["a69f76ec549374252e1d8d475ccd8b69be7c72d9", "d363ce7d5f4a374530ccde7548bb52b31f242a12", "fa8568c5c4f30ca3fd4bda6124f9d0fb54e54d75", "7737e532ffab9166fa2a2310dc472f2de39289df"]
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
knowledge-graph → grounded-answer pipeline, shipped across commits
7737e532 (feature), fa8568c5 (rustfmt/clippy repair, no behavior
change), d363ce7d (review repair: unknown-kind citation tokens preserved
literally in `validate_entity_refs`), and a69f76ec (review repair:
corrected the `short_sha` unicode test expectation to the char-based
value — see Test evidence):

- **Backend** — `POST /api/v1/workspaces/:id/explorer-views/ask`
  (`ask_explorer` in `api/explorer_views.rs`): the LLM prompt is grounded
  in the workspace's live graph — nodes with spec linkage and commit/agent
  provenance (`created_sha`, `last_modified_sha`, `last_modified_by`,
  recency-ranked, capped), edge summaries, and recent architectural
  timeline deltas — via the `explorer-ask` prompt template
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
  resolves the repo scope and runs the task-186 structural prediction
  machinery, returning the impact overlay data for ghost rendering.
- **Frontend** — `ExplorerChat.svelte`: chat panel opened from the Ask
  input in the control bar, conversation history in component state (not
  persisted), responses render validated `[entity:...]` citations as
  clickable links (commit → git, spec → inline open, node → canvas
  highlight, agent → provenance), and apply returned view specs /
  predictions to the canvas.
- **Tests** — server: unit tests for `validate_entity_refs` and
  `short_sha` (pure functions, probe-run this round — see below) plus
  tokio integration tests for the ask endpoint (503 without LLM,
  membership/rate-limit/history-validation paths). Frontend: vitest
  component tests for panel rendering, citation link rendering,
  follow-up context, and view-spec application.

### This round's repair (a69f76ec)

Review round b5c63ebe found the shipped
`short_sha_never_panics_on_short_or_unicode_input` test asserted the
first 7 **bytes** of `"déadbeef"` (`"déadbe"`), while the production
`short_sha` is `chars().take(7)` and returns `"déadbee"` — the
expectation matched the forbidden byte-truncation semantics, so the
suite failed at runtime. Fixed the assertion to the char-based value.

### Test evidence

- The sandbox build wrapper blocks gyre-server test builds (4 GiB
  budget; server tests belong to the independent verifier), so the two
  pure unit tests were executed via a verbatim rustc probe — the exact
  production functions and test bodies extracted unchanged from this
  tree (`serde` derives only stripped): **both pass**. A negative
  control with the byte-slicing implementation fails (panics), proving
  the test discriminates the regression it guards. Evidence:
  `/tmp/stage/review-evidence/probe3/` (gen.sh, verbatim_test.rs,
  probe3.log, negative_control_result.log).
- `cargo check -p gyre-server --all-targets` passes (test module
  included), exit 0.
- Frontend focused suite run this round: `npx vitest run` over
  ExplorerChat.test.js, ExplorerChat-component.test.js,
  ExplorerChat-streaming.test.js, ConversationalExplorer.test.js,
  GeneratedViewChat.test.js — **5 files, 146 tests, all passed**
  (exit 0; log in `/tmp/stage/review-evidence/probe3/frontend_chat_tests.log`).
- **Verifier must run:** `cargo test -p gyre-server --lib
  api::explorer_views` — this is the suite the review rounds flagged;
  it now compiles and its pure tests are probe-verified, but the full
  run (including the tokio ask-endpoint tests) requires the verifier's
  build budget. Frontend: `cd web && npm ci && npm test`.
