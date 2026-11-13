# Review: task-187 — Conversational Exploration (AI Q&A grounded in knowledge graph)

**Reviewer:** Independent review agent
**Round:** R1
**Spec ref:** system-explorer.md §8 Conversational Exploration
**Comparison base:** 41f4d56e8419e24a2a66c7fab33e79445d4a656b
**HEAD reviewed:** 6ca1ef0df904cf95b3a4941098a04fc307078b5c (impl commit 7737e532)
**Status:** complete

---

## Findings

None blocking. Non-blocking observations recorded below (§Observations).

### Acceptance-criteria evidence

**Panel accessible from Ask input in control bar** — `ConversationalExplorer.svelte`
is the third Explorer chat-column mode (`chatMode === 'explore'`), opened from the
control bar's view selector via a new "Ask the graph…" option
(`openConversationalExplorer`, ExplorerView.svelte:1444) sitting next to the
existing "Generate view…" entry, plus a chat-column tab. Verified by the extended
`ExplorerDefaultViews.test.js` option list (passed).

**Questions sent with knowledge-graph context to LLM** — `build_ask_grounding`
(explorer_views.rs) queries the real graph store per repo (`list_nodes`,
`list_edges`, `list_deltas`), recency-ranked, capped (400 nodes / 300 edges /
20 deltas / 10 repos), carrying per-node `spec_path`, `created_sha`,
`last_modified_sha`, `last_modified_by`, and timeline deltas with commit/agent/
spec refs. The prompt goes through the template chain (DB override →
`specs/prompts/explorer-ask.md` → `PROMPT_EXPLORER_ASK` fallback); the question
and history travel as the user prompt only (prompt-injection containment, same
rule as generate). `ask_explorer_grounded_prompt_reaches_llm` captures the mock
LLM's system prompt and asserts node name, spec path, and agent provenance are
present with no unsubstituted `{{` placeholders.

**Grounded, traceable responses** — `validate_entity_refs` parses every
`[entity:<kind>:<id>]` token and resolves it against the same graph index the
prompt was built from; hallucinated ids are stripped to bare text and excluded
from `entity_refs`. Unit test + endpoint test (`ghost-node` case) cover this.
Pure-function unit test `validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens`
covers malformed/unknown-kind tokens; slicing is char-boundary-safe.

**Clickable entity references; click → navigate/highlight** —
`parseSegments` renders tokens with ref metadata as `<button class="entity-ref …">`
(distinct per-type colors); node → detail panel + camera focus
(`conversationOpenNode`), spec → inline editor, commit → git remote commit page
or copied SHA, agent → highlight nodes it last modified. Vitest clicks each ref
type and asserts the right callback fires with the right id.

**Follow-up context** — client-owned history (max 20, server-rejects-more,
roles validated user/assistant) replayed into the user prompt. Tested both
sides (frontend history replay; server 21-entry rejection and system-role
rejection).

**"Show me X" → canvas view** — LLM `view_spec` passes through the SAME
validation chain as the generate endpoint (`parse_and_validate` grammar,
`validate_repo_ownership`, `enforce_flow_trace_backing` with in-answer note);
invalid specs are dropped to null. Auto-applied via `applyGeneratedViewSpec`
plus a view card with an Apply button. Tested (valid passthrough + invalid
drop).

**"What would happen if…" → structural prediction** — the prediction core was
extracted from `POST /repos/:id/graph/predict` into
`run_structural_prediction` (graph.rs) and shared by both endpoints (no
divergent copy; existing `predict_graph_*` endpoint tests still cover the
refactored core). Repo scope resolution validates the caller-supplied
`repo_id` against the workspace (`validate_repo_scope`, same helper as
generate) or falls back to the single-repo workspace. Frontend renders ghost
overlays through the shared `predictionOverlays` mapping. Tested with mock.

### Security / invariant checks (all exit 0)

- `check-arch.sh`, `check-abac-route-registry.sh` (new route
  `/api/v1/workspaces/:id/explorer-views/ask` mapped to
  `explorer_view`/`generate`, registered before `/:view_id`; axum matches the
  literal segment so the resolver lookup is exact),
  `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`,
  `check-task-commit-attribution.sh` (impl commit 7737e532 listed in
  frontmatter), byte-slice, ref-resolution fail-open, fabricated-scope,
  lossy-secret, forwarded-header, in-memory-state, relative-path,
  dead-message-kinds, migration scripts, forged-scope-fields.
- `builtin:developer-generate-access` covers `explorer_view` + `generate`,
  so the new endpoint is callable by Developer+ exactly like `/generate`.
- Membership check, sliding-window rate limit (10/min shared with generate),
  `llm_query` cost + budget recording with template SHA — identical pattern
  to the generate endpoint.

### Probes run (evidence in /tmp/stage/review-evidence)

1. `npx vitest run ConversationalExplorer.test.js ExplorerDefaultViews.test.js`
   → 12/12 passed (after `npm ci`).
2. Mutation probe A: disabled the canvas-action dispatch
   (`onApplyView`/`onPrediction`/`onHighlightNodes`) in `send()` → 2 tests
   fail. Restored; 6/6 pass.
3. Mutation probe B: disabled ref-link rendering in `parseSegments` → 3 tests
   fail. Restored; tree clean.
4. `cargo check -p gyre-server` → Finished, no warnings/errors (cold, 11m44s).
5. All 15 mechanical invariant scripts → exit 0.

### Verifier must execute (not run here — gyre-server test compilation is the
verifier's stage per the hard 4 GiB budget)

- `cargo test -p gyre-server explorer_views::tests::ask_explorer` (5 endpoint
  tests: grounding validation, view-spec passthrough/drop, prediction trigger,
  prompt grounding, history limits + LLM/membership)
- `cargo test -p gyre-server explorer_views::tests::validate_entity_refs_`
- `cargo test -p gyre-server explorer_views::tests::short_sha_`
- `cargo test -p gyre-server graph::tests::predict_graph` (refactor regression)
- `cargo test -p gyre-server abac_middleware` (route registry / policy tests)
- Standard full-suite + frontend run per repo convention.

### Observations (non-blocking)

1. `conversationOpenNode` degrades to a warning toast when the referenced node
   is not in the currently loaded canvas graph (e.g., workspace-scope Boundary
   level 1 shows repo nodes, not code nodes). The spec allows "navigate to the
   entity or highlight on canvas"; this is a scoped-degradation, not a false
   navigation.
2. `build_ask_grounding` surfaces the node's primary `spec_path` only, not the
   plural `spec_paths` list — secondary linked specs cannot be cited. No
   correctness risk (validation is allow-list based); the task instructions
   name `spec_path` as the provenance field.
3. Generated-output note: `cargo check`'s build.rs rebuilt `web/dist/` with
   new content hashes in this sandbox; restored to the committed state. The
   bundle is the verifier's owned artifact and was not reviewed as source.
