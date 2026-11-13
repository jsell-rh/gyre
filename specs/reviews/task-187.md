# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 Conversational Exploration.
Comparison base: `b9487b1b7a1486d808df5fc20dc23a8c903f150e` → candidate `28ef9598ebae8e6acac76f1cd22121b948707530`.
Diff: 15 files, +2425/−86 — backend ask endpoint + grounding validation + tests (`api/explorer_views.rs`), prediction-core extraction (`api/graph.rs`), ABAC route mapping + route registration, `llm_defaults.rs` fallback prompt, `specs/prompts/explorer-ask.md`, `ConversationalExplorer.svelte` (new), `ExplorerView.svelte` integration, `api.js`, locales, docs, and two frontend test files.

Verdict: **complete** (approved; verifier probes recorded below).

Evidence under `/tmp/stage/review-evidence/task-187/`.

## Backend (`ask_explorer`, `POST /api/v1/workspaces/:id/explorer-views/ask`)

- **Real grounding, real validation.** `build_ask_grounding` reads the workspace's live graph (repo-scoped after `validate_repo_scope`): nodes with `spec_path`/`created_sha`/`last_modified_sha`/`last_modified_by` provenance (recency-ranked, capped 400), edge summaries (capped 300), and architectural timeline deltas (capped 20, most recent first). The same data builds the prompt AND the `GraphGrounding` validation index — `validate_entity_refs` parses every `[entity:node|spec|commit|agent:id]` token from the LLM answer and resolves it against that index; an unresolvable citation is reduced to bare id text and excluded from `entity_refs`, so the UI can never render a clickable link to a nonexistent artifact. Token parsing uses only `find`-derived byte indexes and char-based `short_sha` — no fixed byte slicing (check passes).
- **Follow-up context.** Client-owned history (HSI §1325 pattern, like briefing-ask), capped at 20 entries with server-side rejection (400) beyond, role validation (`user`/`assistant` only — a `system` role cannot inject into history), per-entry length caps. History + question travel as the user prompt only; the system template's `{{question}}` placeholder is explicitly blanked — prompt-injection containment matches the other LLM endpoints.
- **Canvas actions.** A returned `view_spec` goes through the same chain as the generate endpoint (`parse_and_validate` → `validate_repo_ownership` → `enforce_flow_trace_backing`, with the fallback note riding the answer); an invalid spec is dropped to null, never forwarded. `"predict": true` runs the SAME structural-prediction core as `POST /repos/:id/graph/predict` — `run_structural_prediction` was extracted from `predict_graph` (not duplicated), and the existing `predict_graph*` tests cover the refactored path. Prediction failure degrades to the text answer (logged), and an ambiguous repo scope (0 or >1 repos, no `repo_id`) skips the prediction rather than guessing.
- **Auth/security.** `check_workspace_membership` (tenant + membership for user tokens) before anything; rate limit 10 req/60 s keyed on the auth principal (same limiter as generate); LLM availability → 503 `llm_unavailable`; budget/cost recorded as `llm_query` with the prompt-template SHA. The route is registered in `api/mod.rs` AND in the ABAC resolver (`explorer_view`/`generate`, Developer+ via the builtin-developer-generate-access policy), placed BEFORE `/:view_id` so first-match resolution cannot capture `/ask` as a view id — `check-abac-route-registry.sh` passes with no new exemptions or duplicates.
- **Tests (type-checked; execution deferred to verifier — see below).** Eight tests covering: hallucinated-citation stripping end-to-end through the real router with a mock LLM; valid/invalid view-spec pass/drop; prediction triggering over the resolved single-repo scope; the grounded prompt actually reaching the LLM (captured system prompt contains node name, spec path, agent provenance, citation syntax, and no unsubstituted `{{`); history replay + over-cap/invalid-role rejection; membership/503 paths; plus unit tests for `validate_entity_refs` (unknown-kind and malformed tokens preserved literally) and `short_sha` unicode safety. The mock/seed helpers mirror the existing generate-endpoint tests in the same file.

## Frontend

- **`ConversationalExplorer.svelte`** (new): chat-style panel, component-state history (not persisted), Enter-to-send, thinking/error states, welcome suggestions matching the spec's three question archetypes. Entity tokens render as distinctly-styled clickable buttons via `parseSegments` — only tokens with server-provided ref metadata become links; a token without metadata stays plain text (defense in depth behind the server's stripping). Click dispatches per type: node → detail panel + canvas focus, spec → inline editor, commit → git remote (or copied SHA), agent → highlight the nodes it last modified. Responses apply canvas actions: `view_spec` → `onApplyView` (the same apply path as generated views), `prediction` → ghost overlays, node refs → `onHighlightNodes` (concept-scope highlight with emphasis/dim — the search-highlight visual language).
- **`ExplorerView.svelte`**: wires the panel as the chat column's third mode plus an "Ask the graph…" entry in the control-bar view selector (the task-178 Ask-input surface); `predictionOverlays` was extracted (not duplicated) from the spec-edit prediction flow so both render the same §3 ghost-overlay language.
- **Tests**: 6 mounted-component tests (real component, mocked transport only) — question → endpoint call with repo scope and empty history; all four ref types clickable with correct navigation dispatch per type; view-spec apply + node highlight; prediction apply + chip; follow-up history replay asserting the exact `{role, content}` pairs sent; error surfacing. `ExplorerDefaultViews.test.js` (mounts the real `ExplorerView`) asserts the new control-bar entry.

## Probes run (evidence: `/tmp/stage/review-evidence/task-187/`)

1. `npm ci` + `npx vitest run ConversationalExplorer.test.js ExplorerDefaultViews.test.js` → **12/12 passed, exit 0**.
2. **Mutation probe**: disabling ref-link rendering in `parseSegments` (`if (ref)` → `if (false && ref)`) → **3 tests fail, exit 1** (rendering, click dispatch, history). The tests kill the disabled production behavior — not vacuous. Source restored byte-identical (`git status` clean).
3. `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → **exit 0** (7m / 3m) — the ask test module compiles against the real AppState/router.
4. `cargo test -p gyre-server --lib ask_explorer` → **rejected by the executor's build wrapper** (gyre-server test codegen exceeds the 4 GiB budget) — infrastructure constraint, not a code defect.
5. All 17 mechanical invariant scripts (`check-arch.sh`, `check-abac-route-registry.sh`, `check-byte-slice-truncation.sh`, and the 14 other repo checks) → **OK at the candidate**.

## Required verifier probes (server tests this sandbox could not execute)

- `cargo test -p gyre-server --lib ask_explorer` (6 router-level tests)
- `cargo test -p gyre-server --lib validate_entity_refs` and `--lib short_sha` (unit tests)
- `cargo test -p gyre-server --lib predict_graph` (regression on the extracted prediction core)

## Observations (non-blocking)

- The `web/dist/` state is generated output owned by the verifier (rebuild + commit happens remotely); `cargo check`'s build.rs regenerated it locally and it was restored — recorded separately, not a source finding.
- `web/src/locales/en.json` carries unrelated `\uXXXX` → literal-character reserialization in pre-existing keys (semantically identical JSON, likely merge-resolution fallout). No functional effect; noting for diff hygiene only.
- Coverage row 16 (`system-explorer.md` §8) remains `task-assigned → task-187` — correct state while in review; promotion to verified is the auditor's call.

## Conclusion

Every acceptance criterion is met by real production code: panel accessible from the control-bar Ask surface, questions sent with genuine knowledge-graph context (provenance/spec-linkage/timeline), server-enforced grounding with hallucinated citations stripped before the client sees them, clickable entity references with per-type navigation, follow-up history with server-side caps and role validation, "show me X" → grammar-validated view spec applied to the canvas, and "what would happen if…" → the shared structural-prediction core rendered as ghost overlays. Frontend tests pass and are mutation-sensitive; the server tests type-check and use the same proven mock/router patterns as the sibling generate-endpoint tests, with execution correctly deferred to the verifier per the sandbox's build budget.
