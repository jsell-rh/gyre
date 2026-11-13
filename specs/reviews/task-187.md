# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration".
Candidate: `c9f53cfc` (base `23d7361f`); product commits `7737e532` (feature),
`fa8568c5` (rustfmt/clippy repair), `d363ce7d` (round-1 repair: unknown-kind
citation tokens), `a69f76ec` (round-2 repair: short_sha test expectation).
Verdict: **approved** — both prior rounds' defects are fixed and verified;
all acceptance criteria hold on independent evidence. Evidence:
`/tmp/stage/review-evidence/r-final/summary.md` (+ `probe5/`).

## Round 3 (this review)

### Prior-round defects — both fixed, independently probe-verified

- **a8fa631b (unknown-kind token):** production `validate_entity_refs` now
  preserves `[entity:unknown:x]` literally (moved to the literal-preservation
  branch, commit d363ce7d); the frontend regex only links the four known
  kinds AND only tokens with server-issued ref metadata, so an unknown-kind
  token can never become an ungrounded clickable reference. Verbatim rustc
  probe of the production fn + the unit test (extracted unchanged from the
  candidate): PASS.
- **b5c63ebe (short_sha expectation):** the test now asserts the char-based
  value `"déadbee"` (commit a69f76ec), matching production
  `chars().take(7)` — the byte-truncation semantics the check forbids are
  what the old expectation encoded. Verbatim probe: PASS. Negative control
  from the prior round (byte-slicing impl panics) already proved the test
  discriminates.

### Acceptance criteria — evidence per criterion

- **Q&A panel accessible from Ask input in control bar** — "Ask the graph…"
  option in the Explorer control-bar view selector (`view-option-ask`) plus a
  third chat-column tab (`chat-mode-explore`), mirroring the task-178
  "Generate view…" entry pattern. Asserted in `ExplorerDefaultViews.test.js`
  (options list includes 'Ask the graph…'); 40/40 ExplorerView-dependent
  tests pass.
- **Questions sent with KG context to LLM** — `ask_explorer` builds the
  prompt from live graph storage: nodes with spec linkage + created/last-
  modified commit SHAs and agent attribution (recency-ranked, capped 400),
  edge summaries, and timeline deltas (capped 20), via the committed
  `specs/prompts/explorer-ask.md` template + `PROMPT_EXPLORER_ASK` fallback.
  Question/history travel as the user prompt only (prompt-injection
  containment, same rule as the other LLM endpoints). Server test
  `ask_explorer_grounded_prompt_reaches_llm` asserts the substituted prompt
  carries the node name, spec path, agent id, and no `{{` placeholders.
- **Responses grounded with traceable entity refs** — `validate_entity_refs`
  checks every `[entity:node|spec|commit|agent:id]` citation against the same
  graph the prompt was built from; unresolvable ids are stripped to bare text
  and excluded from `entity_refs` (which carry resolved labels). Test seeds a
  real node via `graph_store.create_node` and asserts a hallucinated
  `[entity:node:ghost-node]` is stripped while all four valid citation kinds
  survive verbatim.
- **Entity refs rendered as clickable links** — `ConversationalExplorer.
  svelte` `parseSegments` renders validated tokens as styled `<button>` links
  (distinct colors per kind); only tokens with server ref metadata link.
  Component test asserts 4 links with correct labels/types. No `@html` — LLM
  output is escaped text (XSS-safe).
- **Click → navigate/highlight** — node → detail panel + canvas focus;
  spec → inline editor; commit → git remote (or copied SHA); agent →
  highlight nodes last-modified by that agent. Component test asserts each
  callback fires with the right id; ExplorerView handlers verified to route
  into the existing tested machinery (concept-scope query, `openNodeDetail`,
  `openSpecEditor`, ghost overlays).
- **Follow-ups maintain context** — client-owned history (component state,
  not persisted) replayed into the user prompt; server caps at 20 turns and
  rejects over-cap/invalid-role/empty entries with 400 before any LLM budget
  is spent. Component test asserts the second call carries the first Q+A as
  history; server test asserts the 400s.
- **"Show me X" updates canvas** — returned `view_spec` passes the SAME
  validation chain as the generate endpoint (grammar `parse_and_validate`,
  `validate_repo_ownership`, `enforce_flow_trace_backing` with an in-answer
  fallback note); invalid specs are dropped to null, never forwarded. Server
  test covers both branches; the applied spec goes through the existing
  `applyGeneratedViewSpec` path.
- **"What would happen if…" triggers predictions** — `predict: true` answers
  run the task-186 prediction core, extracted in this diff into
  `run_structural_prediction` and shared with `POST /repos/:id/graph/predict`
  (no duplicated divergent copy; behavior-preserving refactor verified by
  diff inspection). Repo scope resolved from request or the single-repo
  workspace; the payload renders as ghost overlays via `predictionOverlays`,
  also extracted from the spec-edit flow (shared, not duplicated). Server
  test asserts the prediction payload is non-empty over the right repo.
- **Tests pass** — frontend: ConversationalExplorer 6/6, ExplorerView-
  dependent suites 40/40 (focused vitest runs, exit 0). Server: `cargo check
  -p gyre-server` exit 0; the two previously-failing pure unit tests pass
  against verbatim production code (rustc probe); the tokio integration
  tests are statically verified compile- and runtime-consistent (helpers,
  struct-literal field sets, error mappings, mock behavior, rate-limit
  budgets all checked — see evidence summary) and MUST be executed by the
  verifier: `cargo test -p gyre-server --lib api::explorer_views`.

### Scope/safety checks

- Route registered before `/:view_id` in both the router and the ABAC
  resolver (first-match safe); `check-abac-route-registry.sh` OK; the route
  runs under middleware ABAC (explorer_view/generate, Developer+ role via the
  builtin-developer-generate-access policy — the policy comment count updated
  five→six LLM endpoints).
- Caller-supplied `repo_id` validated against the caller's workspace
  (`validate_repo_scope` → 403/404) before it can scope the prompt or the
  prediction — no cross-workspace graph leakage.
- Rate limit (10/60s per user+workspace), `llm_query` cost entry, and budget
  usage recording with prompt-template SHA match the generate endpoint.
- `check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-abac-exempt-handlers.sh`, `check-inert-enforcement.sh` all pass;
  `check-task-commit-attribution.sh` passes at the candidate commit (its
  failure against the working tree is the controller's `commits: []` frontmatter
  reset, not a candidate defect).
- No test inflation found: the component tests assert on rendered DOM and
  dispatched callbacks (they fail if linking, history replay, or canvas-action
  dispatch breaks); the server tests drive the real router with real mem-store
  graph data and a mocked LLM (the repo's established pattern for LLM
  endpoints — the grounding validation itself is exercised against real
  seeded nodes, not mirrors of the validator).

### Non-blocking observations

- The task file's committed "Shipped" section names `ExplorerChat.svelte` as
  the frontend panel; the actual component is `ConversationalExplorer.svelte`
  (docs/ui.md and docs/api-reference.md name it correctly). Doc slip in the
  task notes only.
- Coverage row (`system-explorer.md` §8) remains `task-assigned`, consistent
  with sibling completed task-186's row — the coverage flip belongs to a later
  pipeline stage.
- Generated-output note: mid-session `web/dist/` churn appeared from a
  background/earlier-stage build (asset hashes match the earlier verification
  build log); restored to session-start state. `web/dist` is verifier-owned;
  not a source defect.

### Verifier must run

1. `cargo test -p gyre-server --lib api::explorer_views`
2. `cd web && npm ci && npm test`

Verdict: **approved** (round 3). The two prior blocking findings are closed
with code + probe evidence; no new defects found.
