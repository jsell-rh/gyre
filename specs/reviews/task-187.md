# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration".
Base `890a70de` → candidate `91dc2432` (product commits `7737e532`, repair `fa8568c5`; docs/process `f693f74b`/`8e0ce95d`/`6ca1ef0d`/`91dc2432`).
Verdict: **needs-revision** — one blocking finding (F1: shipped server test suite fails, unexecuted by the implementer).

## Round 1 (2026-11-13)

### Probes run (evidence in /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (7m19s, log `cargo-check-gyre-server.log`).
- `cargo test -p gyre-server --lib api::explorer_views` → **blocked by the sandbox build wrapper** (4 GiB cap; wrapper output saved). Deferred to the independent verifier — and F1 below predicts its failure.
- Standalone logic probe: exact copy of the production `validate_entity_refs`/`short_sha` (extracted verbatim from `explorer_views.rs` at 91dc2432) + exact replication of the shipped unit test, compiled with rustc and executed (`probe/replicate_test.rs`, `probe/main.rs`): **panics at the shipped test's own assertion** (`cleaned.contains("[entity:unknown:x]")` fails — the function reduces an unknown-kind token to its bare id `x`). Exit 101.
- `cd web && npm ci && npx vitest run src/__tests__/ConversationalExplorer.test.js` → **6/6 passed**.
- Regression vitest: `ExplorerDefaultViews` + `GeneratedViewChat` + `ExplorerChat` → **121/121 passed**.
- Mutation probes on `ConversationalExplorer.svelte` (restored after each; file clean vs HEAD afterwards): disabling `onHighlightNodes(nodeIds)` → 1 test fails; disabling ref-link rendering (`if (ref)` → `if (false)`) → 3 tests fail. The frontend tests are load-bearing, not self-confirming.
- Mechanical checks (all exit 0): check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-abac-context-parity, check-api-auth, check-mcp-write-tools, check-byte-slice-truncation, check-unbounded-external-http, check-inert-enforcement, check-forged-scope-fields, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-relative-path-defaults.
- Pre-existing failures NOT introduced by the candidate (verified: same failures with the working tree at base `890a70de` or in files the diff never touches): check-llm-endpoint-hygiene (3 `DISCARDED MAX_TOKENS`, among them the `graph.rs` line merely moved by this refactor — base fails identically at line 2105), check-dead-components (4 pre-existing dead components, none from this diff), check-assertionless-tests / check-comparative-test-claims-js (exit-2 script quirks, zero violations printed).

### Verified working (no findings)

- **Route + authz**: `POST /api/v1/workspaces/:id/explorer-views/ask` registered in `api/mod.rs` (static segment before `/:view_id`); ABAC `RouteResourceMapping` added before the `/:view_id` entry (first-match resolver) mapping to `explorer_view`/`generate` — the same Developer+ gate as `/generate`; registry and exempt-handler checks pass. Handler additionally enforces `check_workspace_membership` and validates caller-supplied `repo_id` against the workspace (`validate_repo_scope` → Forbidden/NotFound), so no cross-workspace graph leakage into the prompt.
- **Grounding is real**: `build_ask_grounding` queries live `graph_store` (nodes with `spec_path`/`created_sha`/`last_modified_sha`/`last_modified_by`, edges, timeline deltas), recency-ranked and capped; the prompt-template chain (DB override → `specs/prompts/explorer-ask.md` → `PROMPT_EXPLORER_ASK`) substitutes it; the user question and history travel only in the user prompt (roles restricted to user/assistant, 20-turn cap enforced server-side with 400 on violation — injection containment).
- **Grounding enforcement**: `validate_entity_refs` validates every `[entity:kind:id]` citation against the same graph index the prompt was built from; hallucinated ids are stripped to plain text and excluded from `entity_refs`. My standalone probe confirms valid tokens survive verbatim, dedup, ordering, labels (node name, spec path, 7-char char-safe SHA), unicode-id safety.
- **Canvas actions**: view specs pass the same grammar + repo-ownership + flow-trace-backing chain as `/generate` (invalid → dropped, not forwarded); "what would happen if…" runs the prediction core extracted from `POST /repos/:id/graph/predict` (`run_structural_prediction` — a genuine deduplication, not a fork; the `predict_graph` tests' behavior is preserved by the refactor).
- **Frontend**: `ConversationalExplorer.svelte` renders chat-style Q&A from the control-bar "Ask the graph…" entry and a third chat tab; entity refs render as styled clickable buttons dispatching per type (node → detail + camera focus, spec → inline editor, commit → git remote/copy, agent → highlight its nodes); node refs highlight via concept-scope query; predictions map through the shared `predictionOverlays` (now used by both prediction flows); history is component-state only. All 25 new i18n keys present. `ExplorerView.svelte` wiring targets existing machinery (`openNodeDetail`, `openSpecEditor`, `ghostOverlays`, `activeViewQuery`).
- Rate limiting (10/min per user per workspace), budget/cost recording with prompt-template SHA — mirrors the generate endpoint exactly.

### Findings

- [F1, blocking — test/code contradiction, suite fails as shipped] `crates/gyre-server/src/api/explorer_views.rs`: the unit test `validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens` (line 3514) asserts at line 3541 that `[entity:unknown:x]` (a well-formed token of an unknown kind) is preserved as literal text, but the production `validate_entity_refs` (line 1032) sends unknown kinds through `_ => None` and the `None` branch reduces the token to its bare id (`x`). The implementer's task notes state the server tests were only compile-checked, never executed — this confirms it. Standalone replication of the exact function + exact test panics on that assertion. `cargo test -p gyre-server --lib api::explorer_views` will fail on it, so the acceptance criterion "Tests pass" is not met. Repair: preserve unknown-kind tokens literally (treat an unrecognized kind like a malformed token) or correct the assertion to expect bare-id reduction — then run the suite.

### Verifier must run (deferred from this sandbox)

    cargo test -p gyre-server --lib api::explorer_views
    cargo test -p gyre-server --lib api::graph::tests
    cd web && npm ci && npx vitest run   # full frontend suite

Expectation: the second command's `validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens` fails per F1 until repaired; frontend suites pass (verified here for the focused + regression files).

### Non-blocking observations (recorded for completeness)

- The endpoint double-charges nothing, but the conversational prediction's second LLM call (`run_structural_prediction`) records no separate cost entry — same as the original `predict_graph`, so consistent with existing behavior.
- `conversationOpenCommit`'s `try { navigator.clipboard?.writeText(sha) } catch` cannot catch the async promise rejection; worst case the copy silently fails. Cosmetic.
- Generated `web/dist/` is stale relative to the new frontend source — owned by the deterministic verifier per the review contract; not a finding.
