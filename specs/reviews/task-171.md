# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (+ Role / Available Data / Output Format / Constraints subsections).
Candidate under review: `2bcdf8ffc752554b130d30b00af2d1fdc3af0f57` (assigned base `7da7bbfff6`, merge repair `c32837bc2` + lifecycle commits `45b8f82cf`/`2bcdf8ffc`).
Verdict: **complete**.

## Round N (rebase-repair candidate 2bcdf8ffc)

Focused test run (independent, `/tmp/stage/bin/cargo`, clean tree):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save builtin developer jwt` → **291 passed, 0 failed** (1067 filtered out, exit 0) — matches the shipped claim. Log: `task171-review-focused-tests.log` + named-subset log `task171-key-regression-tests.log` (15/15, explicitly including both merge-conflict-resolved tests `developer_jwt_can_generate_on_llm_endpoints` and `builtin_policy_seeding_fails_closed_on_store_error`, prompt-template git-tree tests, all three `*_rate_limited_after_10_requests`, `generate_explorer_view_charges_llm_query_budget_record`, all five SSE `error`-event tests).
- `cargo test -p gyre-adapters --lib -- no_duplicate_migration_version` → 1 passed (three `000056` dirs are distinct Diesel versions — the date prefix differentiates; script + guard both OK).
- Mechanical gates on the candidate tree: `check-abac-route-registry.sh` OK (now scans the production region only, honors its own FAIL exit); `check-migration-versions.sh` OK; `check-unbounded-external-http.sh` OK with **0 exemptions** (the 5 legacy `rig_vertexai.rs` sites were actually fixed with real `.timeout(...)` — 120s Vertex, 10s OAuth exchanges, 5s GCE metadata — rather than re-exempted; FROZEN count lowered 5→0, a strengthening); `check-arch.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`, `check-task-commit-attribution.sh` all OK.
- Tree verified clean (`git status`) before and after every mutation probe; all mutations reverted.

Mutation probes (each reverted and re-verified clean; logs under review-evidence):

- **Rate limiting disabled** (`check_rate_limit` Err branch killed): all three endpoint rate-limit tests FAILED (`generate_explorer_view_…`, `briefing_ask_…`, `assist_spec_…` — 200 vs 429). Killing tests.
- **`validate_prompt_path` enforcement removed** from `save_prompt`: both path-guard tests FAILED (first rejection shape `specs/system/vision.md` → 201 vs 400). Killing tests.
- **`record_llm_budget_call` gutted** (early return): both budget tests FAILED (`generate_explorer_view_charges_llm_query_budget_record`, `record_llm_budget_call_increments_counters_and_persists_record` — 0 records vs 1). Killing tests.

Verified working (no findings):

- **SSE contract**: all three endpoints return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events; `error` events fire on connection failure AND mid-stream chunk failure (a truncated stream can no longer masquerade as `complete` — the old `filter_map(|r| r.ok())` drop is gone); tested against mock adapters with injected failure modes (`MockLlmAdapter::failing`, `stream_error_after`).
- **Prompt templates**: `resolve_prompt_template` = DB override → `specs/prompts/<fn>.md` from the repo's real git tree (real git2 read + branch-head SHA as provenance, verified in tests asserting a 40-hex SHA) → hardcoded fallback. `substitute_template` does exact-brace `{{var}}` substitution (no partial-name clobbering, no recursive expansion). `save_prompt` commits directly to the default branch and returns the commit SHA; the review-gate bypass is confined by `validate_prompt_path` (relative, plain components only, strictly inside `specs/prompts/`, ≥1 file component) — mutation-verified.
- **Model selection**: config override → workspace `llm_model` → `GYRE_LLM_MODEL` → caller fallback. The rebase-repair merge resolution in `resolve_llm_model_with_default` correctly preserves BOTH task-070's caller-supplied `claude-sonnet-4-6` explorer-SDK fallback and task-171's workspace `llm_model` lookup — read against both parents; no regression.
- **Token limits**: `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000 defaults) plumbed through `predict_json`/`stream_complete` (port signature extended; mock + Vertex adapters honor it; tests cover defaults and env override).
- **Rate limiting**: 10 req/60 s sliding window keyed on auth-context principal (`user_id`, else `agent:<id>` — not the mutable display name), in-handler after auth, 429 + `Retry-After` via `ApiError::RateLimited`, 60 s inactivity eviction by the pre-existing background task. `prompts/save` and invalid-request paths correctly do not consume LLM budget.
- **Budget charging**: `llm_query` CostEntry + `BudgetCallRecord` audit row (new `BudgetCallRepository` port with mem/SQLite/Postgres adapters, mem duplicate-id guard in code per port contract) + workspace/tenant counter increments feeding `check_spawn_budget` enforcement; prompt-template git SHA persisted (portable `ALTER TABLE … ADD COLUMN` migration). Enforced in all three REST handlers and the MCP `specs/assist` path (MCP parity: same template resolution, meta-spec binding, rate limiter principal, budget recording).
- **ABAC**: resolver entries for all four routes (`explorer_view`/`workspace`/`spec` with `generate` override), new `developer-generate-access` builtin Allow (Owner/Admin/Developer), Developer-JWT regression test covers all four routes + Viewer denial. Cross-workspace `repo_id` rejected on input (`validate_repo_scope`) and nulled with fallback when hallucinated by the LLM.
- **Grounding**: explorer-generate prompt carries question + node types/counts + view-spec grammar; briefing-ask carries real briefing JSON, history (≤20, rejected above), sources; specs-assist carries spec content (draft override), graph context, and `{{meta_spec_context}}` from the workspace's pinned meta-spec set. User input travels as user prompt only; template `{{question}}`/`{{instruction}}` placeholders blanked (injection containment).

Minor (not blocking, recorded for completeness):

- SSE responses are buffered: the handlers consume the whole LLM stream before returning the `Sse` response, so `partial` events arrive in one flush rather than spread over the generation wall-clock. The spec's observable contract (content type, event names, per-chunk partial payloads, terminal complete/error) is fully met; delivery timing is not specced. Explorer-generate emits a single synthetic `partial` before `complete` (its output is a structured spec via `predict_json`, so there is no natural token stream).
- No dedicated test for the workspace-`llm_model` step of the model-resolution chain (implemented, trivially correct by inspection; the env/fallback and token-limit steps are tested).
- Shipped-notes wording "budget charging … enforced … plus the `prompts/save` path" overstates slightly: `save_prompt` records no budget call — correct, since it makes no LLM call and the spec charges LLM calls only.

Evidence: `/tmp/stage/review-evidence/` (focused-tests, key-regression subset, three mutation logs, migration guard log).
