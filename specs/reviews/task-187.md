# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration" — AI-assisted Q&A where answers are grounded in the knowledge graph (provenance records, spec linkage, architectural timeline), every claim references a concrete artifact rendered as a clickable link, follow-ups carry conversation context, "show me X" updates the canvas, and "what would happen if…" triggers structural prediction.

Commits under review (base `ccfd4bc5` → candidate `82f2b57c`): `7737e532` (feature), `fa8568c5` (rustfmt/style repair), `d363ce7d` (unknown-kind citation tokens preserved literally), `a69f76ec` (char-based `short_sha` expectation), `bdf9f79f` (verification repair), plus docs/process commits. Cumulative diff: 15 files, +2427/−86 (`git diff ccfd4bc5..82f2b57c`).

Verdict: **complete** (approved). Probe evidence under `/tmp/stage/review-evidence/`.

## What was verified

### Backend — `POST /api/v1/workspaces/:id/explorer-views/ask` (`ask_explorer`)

- **Grounding is real storage, not a stub.** `build_ask_grounding` reads the workspace's (or repo-scoped) live graph: active nodes with `spec_path`, `created_sha`, `last_modified_sha`/`last_modified_by` provenance (recency-ranked, capped 400 nodes / 300 edges / 20 timeline deltas / 10 repos), edge summaries, and architectural timeline deltas with commit/agent/spec attribution — all from `state.graph_store` / `state.repos`.
- **All three prompt-template paths carry the graph context.** DB override → `specs/prompts/explorer-ask.md` in the repo git tree → hardcoded `PROMPT_EXPLORER_ASK`. The fallback previously lost `{{graph_context}}` (caught by pipeline verification on the prior candidate `c9f53cf`, durable finding); repair commit `bdf9f79f` adds the context section to the fallback — verified in the committed diff. The user's question and history travel as the user prompt only (never the system template); history roles are restricted to `user`/`assistant` (no system-role injection).
- **Grounding enforcement is real validation, not structural theater.** `validate_entity_refs` parses every `[entity:<kind>:<id>]` token from the answer and resolves it against the same index the prompt was built from. Unresolvable citations are stripped to bare id text and excluded from `entity_refs` (so the UI can never render a clickable reference to a nonexistent artifact); unknown-kind tokens are preserved literally (the frontend regex only links the four known kinds, so they render as plain prose). Pure-logic test `validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens` covers valid/hallucinated/unknown-kind/unterminated tokens, ordering, and dedup. `short_sha` is char-based (no fixed byte slicing — `check-byte-slice-truncation` passes, unicode case tested).
- **Scope safety.** `check_workspace_membership` first (cross-tenant → 403 before any LLM budget); caller-supplied `repo_id` validated against the workspace (`validate_repo_scope` → 403/404 on foreign/unknown); the grounding, view-spec ownership check, and prediction all run inside that validated scope. Rate limit (10/60s per user per workspace, same limiter/principal as generate), `llm_query` CostEntry + `LlmCallUsage` with prompt-template SHA — matching the generate endpoint's budget pattern.
- **Canvas actions go through the existing validation chains.** A returned `view_spec` passes the same grammar (`parse_and_validate`), repo-ownership, and flow-trace-backing enforcement as generated views; invalid specs are dropped to null with the answer still delivered. `predict: true` runs `run_structural_prediction` — the prediction core extracted from `POST /repos/:id/graph/predict` so both surfaces share one implementation (no divergent copy); the graph.rs refactor preserves the original endpoint's behavior (spec-edit framing note moved to the `system_note` slot verbatim). Best-effort degradation when no single repo scope resolves.
- **ABAC.** Route registered in the `ResourceResolver` before `/:view_id` (first-match would otherwise capture "ask" as a view id), same `explorer_view`/`generate` action as the other LLM endpoints; `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass on the candidate tree.

### Frontend — `ConversationalExplorer.svelte` + `ExplorerView.svelte`

- Chat panel opened from the control bar's view selector ("Ask the graph…", same pattern as task-178's "Generate view…") and via a third chat-column tab; conversation history in component `$state` only (not persisted), client-capped at 20 turns.
- Entity refs render as clickable inline links **only when the server resolved them** (frontend token parser requires matching ref metadata — a stripped hallucination cannot become a link); styled distinctly per kind; click dispatches per type: node → detail panel + canvas focus, spec → inline editor, commit → git remote (mirror_url) or copied SHA, agent → highlight nodes the agent last modified (`last_modified_by` exists in the graph API payload, `graph.rs:99`).
- Node references in a response highlight on the canvas via a concept scope seeded by node ids with empty `expand_edges` — verified against `view-query-scope.js` semantics: empty `expand_edges` yields no edge expansion, so the scope matches exactly the referenced nodes (emphasis + dim).
- "Show me X" → validated view spec auto-applied via the same `applyGeneratedViewSpec` path (plus an Apply card); "what would happen if…" → prediction payload mapped to ghost overlays through `predictionOverlays`, now shared with the spec-edit prediction flow (deduplicated from a previously inlined copy).
- No `{@html}` anywhere in the new component — all LLM-derived text renders via text interpolation (no XSS surface from model output).

### Probe evidence (`/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server` — exit 0 (8m13s cold; log kept).
- `cd web && npm ci && npx vitest run` (full suite) — **112 files, 2257 passed, 41 skipped, 0 failed** (`vitest-full.log`); focused run of `ConversationalExplorer.test.js` + `ExplorerDefaultViews.test.js` — 12 passed (`vitest-conversational.log`).
- Mechanical gates on the candidate clean worktree at `82f2b57c`: every AGENTS.md-mandated check passes (arch, abac-route-registry, abac-exempt-handlers, mem-port-contracts, inert-enforcement, task-commit-attribution, migration-versions, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults, migration-sql-portability, forged-scope-fields, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, mcp-write-tools). The advisory checks that fail do so **identically on the assigned base** (verified by running the full check set in a base worktree at `ccfd4bc5` and diffing logs — same failing sets, same violation counts; e.g. `check-llm-endpoint-hygiene`'s 3 violations are pre-existing `signal_chain.rs`/`graph.rs` max_tokens discards that predate this task, only line-shifted).
- Server tests could not be executed in this sandbox: the executor's cargo wrapper rejects gyre-server test builds ("code generation plus the model exceeds 4 GiB", exit 64 — `cargo-test-ask.log`). Compensating evidence: the pipeline's prior full `cargo test --all` on direct ancestor `c9f53cf` (durable finding) ran 1916 tests with exactly 2 failures — `ask_explorer_grounded_prompt_reaches_llm` and `ask_explorer_requires_llm_and_membership` — and the candidate's only delta on those code paths is repair commit `bdf9f79f`, reviewed line-by-line: (1) the fallback template now carries `{{graph_context}}` (all five placeholders substituted; `VIEW_SPEC_GRAMMAR` contains no `{{`, so the no-unsubstituted-variable assertion holds), and (2) the no-LLM test now builds an explicit `llm = None` state (mirroring `app_no_llm()`) instead of the echo-mock `app_with_workspace()`, plus the foreign-tenant 403 the test name promises.

### Required verifier probes (recorded; not runnable in this sandbox)

1. `cargo test -p gyre-server --lib api::explorer_views` — the ask suite, including the two repaired tests.
2. `cargo test --all` — workspace gate (the candidate merges the updated base `ccfd4bc5` past the previously verified `c9f53cf`; integration of unrelated base changes needs the full run).
3. `cd web && npm ci && npm test` — already run in review (pass), re-run to bind the published tree.
4. `bash scripts/check-cli-spec-parity.sh` — see transient note below.

## Non-blocking observations

- **History-replay server test asserts validation, not prompt delivery.** `ask_explorer_sends_history_and_rejects_oversized` proves cap/role rejection and 200 on valid history, but the mock adapter captures only system prompts, so it cannot assert the history reached the LLM user prompt. The code path is a direct string build (`user_prompt` in `ask_explorer`), and the frontend test proves the history is sent. Minor coverage gap, not a defect.
- **Degenerate prediction mock.** In `ask_explorer_predict_triggers_structural_prediction` the same mock factory serves the ask call and the nested prediction call, so the "predictions" payload is the ask JSON itself. It proves the wiring (predict flag → shared prediction core → response payload), which is this endpoint's responsibility; prediction content quality is owned by the graph-predict suite.
- **One unreproduced transient.** An early loop run logged a `check-cli-spec-parity.sh` failure (`max_priority` param) that never reproduced: 30+ consecutive passes across the main checkout, a clean candidate worktree, and a base worktree; the task diff touches no CLI client or Query-struct file, and the check passes on both base and candidate. Recorded for the verifier to re-run (probe 4 above).
- **Pre-existing working-tree state.** The checkout carries an unstaged revert of `specs/tasks/task-187.md` (progress → `not-started`, commits → `[]`) that predates this review session; the committed candidate has correct frontmatter and `check-task-commit-attribution.sh` passes on the clean candidate tree. Not modified by review (specs are out of bounds).
- **Generated `web/dist/`.** Unchanged between base and candidate in this diff. A `cargo check` probe triggered build.rs to regenerate dist assets as a side effect; the tree was restored to the committed state afterwards (git status clean except the pre-existing task-file revert above).

## Acceptance criteria

- Conversational Q&A panel accessible from Ask input in control bar — **yes** (view-selector option + chat tab, task-178's established Ask-input pattern).
- Questions sent with knowledge graph context to LLM — **yes** (verified prompt construction; all template paths grounded after `bdf9f79f`).
- Responses grounded with traceable entity references — **yes** (server-side citation validation against the live graph; hallucinated refs stripped).
- Entity references rendered as clickable links — **yes** (only server-validated refs link).
- Click entity reference → navigate/highlight — **yes** (per-type dispatch verified in component tests).
- Follow-up questions maintain conversation context — **yes** (client-owned history, server-validated, replayed into the user prompt).
- "Show me X" responses update the canvas view — **yes** (grammar-validated view spec, auto-apply + Apply card).
- "What would happen if…" responses trigger structural predictions — **yes** (shared prediction core, ghost overlays).
- Tests pass — **frontend verified in review (full suite)**; server suite compile-checked (`cargo check` exit 0) with prior full-run evidence and the repair verified by inspection — verifier probes 1–2 recorded above.
