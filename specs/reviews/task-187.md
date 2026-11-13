# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration".
Candidate under review: `fa2ccdad` (assignment base `603cc2f`). This is the
rebase-repair re-review after the round-1 merge-conflict finding (published head
`850ece34` conflicted with main; resolved in `f7cfd96d`).
Verdict: **approved** — no blocking findings.

## Scope integrity

`git diff 603cc2f..fa2ccdad -- ':!web/dist'` touches exactly the task-187
surface: `api/explorer_views.rs` (+1110), `api/graph.rs` (prediction-core
extraction), `abac_middleware.rs` (route mapping), `api/mod.rs` (route),
`llm_defaults.rs` (fallback prompt), `specs/prompts/explorer-ask.md` (new
template), frontend (`ConversationalExplorer.svelte` new, `ExplorerView.svelte`
wiring, `api.js`, locales, tests), docs, and the task file. Against the
previously published head `850ece34`, the candidate's diff in every task-187
source file is **empty** (only `web/dist` and the task file differ) — the
merge-repair preserved the implementation exactly; the `api/graph.rs` changes
relative to `850ece34` are main's task-183 `?at=` timeline work plus the
behavior-preserving extraction of `run_structural_prediction` (verified
line-by-line against base: same template resolution, same graph context, same
system-note handling).

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → exit 0 (9m09s; log `cargo-check-gyre-server.log`).
- `bash scripts/check-abac-route-registry.sh` → OK.
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-exempt-handlers.sh` → OK (75 handlers).
- `bash scripts/check-{byte-slice-truncation,relative-path-defaults,inert-enforcement,scope-literal-defaults,in-memory-state-stores,unbounded-external-http}.sh` → all exit 0.
- `python3 scripts/dev-attribution.py task-187` (dry-run, no commit) → exit 0.
- `cd web && npm ci && npx vitest run src/__tests__/ConversationalExplorer.test.js src/__tests__/ExplorerDefaultViews.test.js src/__tests__/ExplorerChat-component.test.js` → **31/31 passed**.
- `cargo test -p gyre-server --lib api::explorer_views` → **blocked by the
  executor's build wrapper** ("gyre-server code generation plus the model
  exceeds 4 GiB"; log `cargo-test-explorer-views.log`, exit 64). This is an
  infrastructure restriction, not a code defect — same restriction the
  implementer documented. **Verifier must run**: `cargo test -p gyre-server
  --lib api::explorer_views`, `cargo test -p gyre-server --lib api::graph`,
  plus the full workspace gates (`cargo test --all`, architecture/CI checks,
  `cd web && npm test`).
- Side effect restored: `cargo check` triggered build.rs, which regenerated
  `web/dist` from current sources (fresh hashed assets). Restored to the
  committed checkpoint state — dist is verifier-owned.

## Verified against the acceptance criteria

- **Panel accessible from Ask input in control bar** — "Ask the graph…"
  option in the view selector (`openConversationalExplorer` → chat mode
  `explore`) plus a third chat-column tab; covered by the updated
  `ExplorerDefaultViews` option-list test.
- **Questions sent with knowledge-graph context** — `build_ask_grounding`
  performs real `graph_store` queries (nodes with `spec_path`,
  `created_sha`/`last_modified_sha`/`last_modified_by`, recency-ranked and
  budgeted; edges; timeline deltas with commit/agent/spec refs) and
  substitutes them into the `explorer-ask` template (DB override →
  `specs/prompts/explorer-ask.md` in the repo tree → hardcoded
  `PROMPT_EXPLORER_ASK` fallback, all three carrying `{{graph_context}}`).
  The grounded-prompt test captures the real system prompt via
  `MockLlmAdapter.captured_system_prompts` and asserts node name, spec
  linkage, and agent provenance are present with no unsubstituted `{{`.
- **Grounded responses with traceable entity refs** —
  `validate_entity_refs` resolves every `[entity:<kind>:<id>]` citation
  against the same graph index the prompt was built from; unresolvable ids
  are stripped to bare text and excluded from `entity_refs`; unknown-kind
  tokens and malformed tokens are preserved as literal prose (never
  clickable). Hand-traced all branches (valid, hallucinated, unknown kind,
  unterminated, empty id) — byte-index slicing is char-boundary-safe (all
  splits at ASCII delimiters from `find`).
- **Clickable refs + navigation** — frontend `parseSegments` uses the same
  token grammar and renders links only for refs carrying server metadata;
  typed dispatch: node → detail panel + canvas focus, spec → inline editor,
  commit → git remote (or copied SHA), agent → highlight nodes the agent
  last modified. Component tests click each type and assert the callback
  payload.
- **Follow-up context** — client-owned history (component state, not
  persisted), capped at 20 turns, replayed into the user prompt; server
  validates roles and rejects over-cap/oversized history with 400 before any
  LLM budget. Prompt-injection containment holds: question/history travel
  only in the user prompt; the committed template's `{{question}}`
  placeholder is explicitly blanked.
- **"Show me X" → canvas view** — LLM `view_spec` passes the same validation
  chain as the generate endpoint (`parse_and_validate`,
  `validate_repo_ownership`, `enforce_flow_trace_backing` with an in-answer
  note on fallback); invalid specs are dropped to null, never forwarded.
  Frontend applies via `applyGeneratedViewSpec` and renders an Apply card.
- **"What would happen if…" → structural prediction** — LLM `predict` flag
  runs the shared `run_structural_prediction` core (extracted from
  `predict_graph`, verified behavior-preserving) over the validated repo
  scope; frontend maps predictions to ghost overlays via the same
  `predictionOverlays` mapping as the spec-edit flow (deduplicated from the
  old inline copy). Prediction failure degrades to the text answer.
- **Security/scope** — `check_workspace_membership` first (cross-tenant →
  403 before LLM budget; asserted by test), `validate_repo_scope` on
  caller-supplied `repo_id` (cross-workspace → Forbidden), ABAC mapping
  registered **before** `/:view_id` (first-match resolver) under
  `explorer_view`/`generate` (Developer+ via the existing builtin
  generate-access policy), rate limit 10/60s keyed on the auth principal,
  budget/cost recording byte-identical to the generate endpoint including
  the prompt-template SHA.

## Test-inflation check

None found. Server tests drive the real router (`build_router` + oneshot
with bearer auth) against the mem-backed stores and a mock at the LLM port
boundary; the hallucination test asserts a fabricated node ref is stripped
while all four real ref kinds survive verbatim; the no-LLM path is tested
with an explicit `llm = None` state. Frontend tests mock at the network
boundary (`api.explorerAsk`) and assert DOM structure, click dispatch, and
the exact history payload — they fail if parsing, dispatch, or follow-up
state breaks.

## Non-blocking observations (recorded for the verifier)

- **Server tests not executed here** (build-wrapper restriction above) —
  the verifier must run them; compilation is proven (cargo check exit 0)
  and the logic was hand-verified.
- **`conversationOpenCommit` opens `repo.mirror_url`-derived URLs** —
  mirror_url is operator-configured (mirror-repo creation path), not
  LLM-controlled; the LLM-controlled SHA is server-validated against real
  graph SHAs and `encodeURIComponent`-encoded. A `javascript:` scheme in
  mirror_url would require a privileged operator to plant it —
  defense-in-depth nit only, consistent with the codebase's existing trust
  model for operator-configured URLs.
- **Coverage matrix not updated** (`specs/coverage/system/system-explorer.md`
  row 16 still `task-assigned`) — consistent with the pipeline pattern: the
  entire matrix is uniformly `task-assigned` at base, including already
  published tasks 178–186; coverage status flips post-verification. Not a
  candidate defect.
- **`web/dist`** — regenerated during review by build.rs and restored; the
  committed bundle state is the verifier's to rebuild and bind.

Verdict: **approved** — the task contract is met with real implementations,
real grounding enforcement, and meaningful tests; the only unexecuted probes
are server tests blocked by the sandbox build budget and explicitly delegated
to the verifier.
