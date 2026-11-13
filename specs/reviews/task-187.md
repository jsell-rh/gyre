# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration".
Candidate: `85fc3143` (base `349a32ac`). Feature commits `7737e532`
(feat) + `fa8568c5`/`d363ce7d`/`a69f76ec`/`bdf9f79f` (repairs); all 8 SHAs
in `commits:` frontmatter verified as ancestors of the candidate, and every
task-187-labeled product commit on the branch is recorded.
Verdict: **approved** — real implementation on every acceptance criterion;
no structural defects, no fake enforcement, no test inflation found.

## Round 1 (this review — fresh model after a gateway-transport failure)

Probes run (checkout at `85fc3143`; evidence in
`/tmp/stage/review-evidence/`):

- `cd web && npm ci && npx vitest run src/__tests__/ConversationalExplorer.test.js src/__tests__/ExplorerDefaultViews.test.js` → **12/12 passed** (exit 0).
- `cargo check -p gyre-server` → exit 0; `cargo check -p gyre-server --tests` → exit 0 (both the ask endpoint and its test module compile clean).
- `scripts/check-abac-route-registry.sh`, `check-arch.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`, `check-byte-slice-truncation.sh`, `check-scope-literal-defaults.sh`, `check-unbounded-external-http.sh` → all OK.
- Full diff inspection of all 15 authored files (see `explorer_views.diff`, `frontend.diff` in the evidence dir); no conflict markers in authored source; base `349a32ac` is an ancestor of the candidate (PR #818 conflict resolved in `e04f6096`).

Verifier must run (server test compilation exceeds this sandbox's 4 GiB
cgroup budget; recorded per stage policy):

- `cargo test -p gyre-server --lib api::explorer_views` (ask endpoint integration + `validate_entity_refs`/`short_sha` unit tests)
- `cargo test -p gyre-server --lib api::graph` (predict-core extraction refactor)
- `cargo test --all`, `cd web && npm test`, CI script gates.

Verified working (no findings):

- **Backend endpoint is real, not a stub.** `POST /api/v1/workspaces/:id/explorer-views/ask` (`ask_explorer` in `api/explorer_views.rs`) builds its prompt from the workspace's live graph storage: `build_ask_grounding` loads real nodes (spec linkage, `created_sha`/`last_modified_sha`/`last_modified_by`, recency-ranked, capped 400/10 repos), real edges, and real timeline deltas — proven to reach the LLM by `ask_explorer_grounded_prompt_reaches_llm` (captured system prompt contains node name, spec path, agent id, citation syntax, and no unsubstituted `{{` placeholders).
- **Grounding is enforced, not audited.** `validate_entity_refs` checks every `[entity:node|spec|commit|agent:id]` token against the same graph the prompt was built from; hallucinated ids are stripped to bare text and excluded from `entity_refs` so the UI can never render a clickable reference to a nonexistent artifact. The endpoint test seeds a real node + delta into the graph store and asserts the ghost citation is stripped while all four valid kinds survive verbatim — it fails if the validator is disabled. Unknown-kind and malformed tokens are preserved literally and can never become links (frontend regex links only the four known kinds).
- **Scope and abuse controls match the sibling LLM endpoints.** `check_workspace_membership` (cross-tenant 403 before any LLM budget), `validate_repo_scope` (cross-workspace repo_id 403/404), the shared 10 req/min sliding-window limiter keyed on the auth principal, `llm_query` cost + budget recording with prompt-template git SHA, and a 503 `llm_unavailable` path — all tested. History is capped at 20 entries with role/content validation (400 on violation, before LLM spend); question capped at 4000 chars. Question and history travel only in the user prompt, never the system template (injection containment).
- **Canvas actions reuse existing machinery rather than divergent copies.** "Show me X" view specs pass the same `parse_and_validate` → `validate_repo_ownership` → `enforce_flow_trace_backing` chain as the generate endpoint (invalid specs dropped to null, tested). "What would happen if…" runs the task-186 prediction core, correctly extracted into `run_structural_prediction` shared with `predict_graph` (refactor is behavior-preserving: same prompt construction, same note placement, same model resolution) — the conversational test asserts the prediction payload carries repo scope and predictions.
- **ABAC registration is correct.** The `/explorer-views/ask` mapping is registered before `/:view_id` (first-match resolver) under `explorer_view`/`generate`, covered by the existing Developer+ generate policy; route-registry script passes with no exemption growth.
- **Frontend is real.** `ConversationalExplorer.svelte`: chat panel with non-persisted history, `parseSegments` renders only server-validated refs as styled clickable buttons (labels/answers render as escaped text — no HTML injection), per-type click dispatch (node → detail + canvas focus, spec → inline editor, commit → git remote or copied SHA, agent → highlight nodes it modified), view-spec card with apply, prediction chip, and automatic node-highlight/view/predict application on response. Entry points: "Ask the graph…" option in the control-bar view selector (asserted in `ExplorerDefaultViews.test.js`) plus a chat-column mode tab. All `conversational_explorer.*` locale keys exist.
- **Tests are meaningful, not inflated.** Frontend tests assert exact API payloads (including the follow-up history array), DOM structure, and per-type callback dispatch — they fail if parsing, history, or dispatch breaks. Server tests drive the real router with a mock LLM and real graph storage, and each kills a specific bug class (hallucinated citation passthrough, invalid spec passthrough, missing grounding, oversized history, missing LLM, cross-tenant access).

Test-quality notes (non-blocking, recorded for completeness):

- The server-side history test asserts acceptance/rejection but not that history text reaches the LLM user prompt (`MockLlmAdapter` captures only system prompts); the replay path is simple, direct code and the frontend test proves the history is sent.
- `ask_explorer_predict_triggers_structural_prediction` asserts the prediction wiring (payload present, repo scope, non-empty) — prediction content is inherently mock-echoed, which is the correct boundary for an LLM-dependent surface.
- The ExplorerView-side canvas glue (`conversationOpenNode` etc.) is covered only through the component's mocked callbacks; it is thin delegation to pre-existing machinery (`openNodeDetail`, `traceFocusRequest`, `ghostOverlays`, `activeViewQuery`).

Generated-output observation (verifier-owned, non-blocking): the committed
`web/dist/index.html` serves `index-CMCAgKSt.js`, which contains the new
`explorer-views/ask` client; a stale orphaned chunk (`index-CHXTZtkz.js`)
also sits in `web/dist/assets`. The verifier's post-integration rebuild
owns the final asset tree.
