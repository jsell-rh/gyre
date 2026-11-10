# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (+ §Role/Available Data/Output Format/Constraints).
Candidate under review: `9d3aef36` (= rebase-repair merge `5ca86c4ec` onto base `b5a872087` + two docs/process commits touching only `specs/tasks/task-171.md`).
Product commits: the 16 listed in the task frontmatter, plus merge commits.
Verdict: **approved** — no findings. Independent probes confirm the merge preserved both the task-171 contract and the base's task-089/task-108 feature sets, and every acceptance criterion is backed by passing, mutation-verified tests.

## Round 4 (this review — rebase repair onto b5a872087)

Candidate identity: `9d3aef36^{tree}` = `5c8971224…`; differs from merge commit `5ca86c4ec` only in `specs/tasks/task-171.md` (shipped-notes/commit-attribution docs). Working tree clean before and after review; the one mutation probe (below) was restored and re-verified green.

Probes run (all logs under `/tmp/stage/review-evidence/task171-round4-*.log`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save builtin developer jwt` → **354 passed, 0 failed** (1284 filtered out) — reproduces the shipped claim exactly.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters budget` → 3 passed (sqlite budget_call round-trip incl. `prompt_template_sha`, tenant-scoped list, duplicate-id guard).
- `cargo test -p gyre-server --lib budget_call` → 2 passed (spawn agent-usage path + mem duplicate-id guard — both sides of the budget unification).
- `cargo test -p gyre-server --test explorer_ws_integration` → 15 passed (3-arg `predict_json` mock).
- Focused subsets: `-- generate` → 21 passed (task-089 flow-layout/refinement tests intact on the merged handler); `-- sse` → 49 passed incl. `generate_explorer_view_streams_sse`, `briefing_ask_with_mock_llm_streams_sse_events`, `assist_spec_with_valid_diff_response_streams_sse`, plus connection-failure and **mid-stream** `event: error` tests on all three endpoints.
- Mechanical: `check-arch.sh`, `check-migration-versions.sh`, `check-mcp-write-tools.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-migration-sql-portability.sh` → all exit 0.
- **Mutation probe (killing-test check)**: `check_rate_limit` rejection branch disabled (`if deque.len() >= limit` → `if false && …`) → **5 tests FAILED** (`explorer_views::generate…rate_limited`, `graph::briefing_ask_rate_limited_after_10_requests`, `specs_assist::assist_spec_rate_limited_after_10_requests`, `llm_rate_limit::rejects_at_limit`, `retry_after_is_nonzero`). Restored from backup; `-- rate` re-run → 50 passed, 0 failed; `git status` clean.

Verified (no findings):

- **Merge preserved both sides.** Task-171 contract layers survive on the merged handler: auth-principal rate limit (`rate_limit_principal` keys on immutable `user_id`, agent tokens on `agent:agent_id`), `validate_repo_scope` on caller-supplied `repo_id`, git-tree prompt template with branch-head SHA provenance (DB override → `specs/prompts/<fn>.md` in git tree → hardcoded fallback), `{{...}}` substitution with unknown vars left verbatim, model + per-endpoint token limits (2000/4000/4000 via `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}`), SSE `partial`/`complete`/`error` including mid-stream failure, LLM-output grammar validation with null-view-spec fallback, hallucinated foreign `repo_id` in generated specs nulled. Base's task-089 features survive: `previous_spec` grammar validation, MR-trace discovery user prompt, `generated_view_explanation`, flow-layout trace backing (`generate_flow_layout_*` tests pass).
- **Budget unification is real, not dropped.** Every caller (explorer generate, briefing/ask, specs/assist REST, `mcp.rs` spec_assist, `spawn.rs` agent usage) funnels through `api/budget.rs::record_llm_call_usage(&LlmCallUsage{…, prompt_template_sha})`; `BudgetCallRecord` persists the SHA through the sqlite adapter (round-trip test asserts a 40-hex SHA), mem adapter keeps the duplicate-id port-contract guard, and workspace+tenant counters increment (task-190 regression test).
- **Migration collision resolved correctly.** Base's `2026-10-09-000056_user_tenant_id` keeps its number; task-171's prompt-SHA migration renumbered to `2026-10-09-000059_budget_prompt_sha` (next unused sequence — `ls migrations | sort | tail` confirms), plain `ALTER TABLE … ADD COLUMN … TEXT` passes the SQL-portability check for both SQLite and PostgreSQL.
- **`mem.rs` conflict resolution compiles and both sides' stores are exercised** (BudgetUsage + ApprovalTransitionError paths tested).
- **`save_prompt` path validation intact** after the merge: `validate_prompt_path` (absolute/`..`/root/prefix/non-UTF-8/outside-`specs/prompts/` rejection, strictly-inside with ≥1 file component) runs before any git write; covered by `save_prompt_rejects_paths_outside_specs_prompts` and `save_prompt_rejects_traversal_but_allows_normalized_subdirs` (mutation-verified in a prior round).
- **ABAC**: all four LLM routes (`explorer-views/generate`, `briefing/ask`, `specs/assist`, `prompts/save`) have production-region `RouteResourceMapping` entries with the `generate` action override; registry script passes with the frozen duplicate count.
- **Rate-limiter background eviction** (`spawn_llm_rate_limiter_cleanup`, `evict_stale_entries` with unit tests) still wired in `main.rs` on the merged tree.

Prior-round repairs (`f7219be2` save_prompt path validation, `7fcbf272` ABAC scan scope + SSE error tests) are present in the merged tree and their tests pass.

## Prior rounds

Rounds 1–3 (candidates `3ff29841`, `e31d4a747` and the `81607e62` rebase onto `fefde2946`) found and repaired: the `save_prompt` review-gate bypass (arbitrary tracked-file overwrite via `prompt_path`), the ABAC registry scan false-positive/invisible-FAIL, and missing SSE error-event coverage. All repaired, with mutation evidence recorded in `/tmp/stage/review-evidence/`.
