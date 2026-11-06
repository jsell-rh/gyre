# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (Role / Available Data / Output Format / Constraints subsections).
Base: `60b3504c`. Candidate: `c6c9390d` (checkpoint merge `f6d870f1` = prior branch tip `1479674a` + base, plus docs/process commits — no code changes this round).
Verdict: **approved**.

## Round summary

This round was a checkpoint continuation after an infrastructure-failed pre-flight (sandbox GitHub TLS handshake, before agent work). The review therefore focused on (a) confirming the merge introduced zero changes to the task surface, and (b) independently re-proving the contract with focused tests and mutation probes.

- Merge-regression check: `git diff --stat 1479674a..c6c9390d -- <all task-surface files>` (llm_helpers.rs, llm_rate_limit.rs, api/{specs_assist,explorer_views,graph,budget}.rs, abac_middleware.rs, gyre-adapters/src/llm, gyre-ports/src/budget_call.rs) → **empty**. The merge added nothing to or removed nothing from the task surface; the only task-relevant post-merge commits are docs/process.
- Focused suite: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save` → **239 passed, 0 failed** (1049 filtered out, exit 0). Log: `/tmp/stage/review-evidence/focused-tests-c6c9390d.log`.
- Mutation probe (rate limit): `LLM_RATE_LIMIT` 10→1000 → all three `*_rate_limited_after_10_requests` endpoint tests FAILED; reverted, green again. Killing tests, not self-confirming.
- Mutation probe (path guard): `validate_prompt_path` short-circuited to `Ok(())` → both `save_prompt_rejects_*` tests FAILED; reverted, green again.
- Mechanical invariants: check-abac-route-registry, check-migration-versions, check-unbounded-external-http, check-mcp-write-tools, check-mem-port-contracts, check-abac-exempt-handlers, check-inert-enforcement, check-task-commit-attribution, check-arch, check-migration-sql-portability → all OK.

## Contract verification (independent)

- **SSE streaming**: all three endpoints (`explorer-views/generate`, `briefing/ask`, `specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events; tests assert the Content-Type on both success and error paths. Error paths are real: LLM connection failure, mid-stream failure (no silent `filter_map(|r| r.ok())` truncation), invalid JSON, and missing required fields each produce `event: error` — seven dedicated tests, all present and passing.
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (branch-head SHA as provenance, verified 40-hex in `prompt_template_loads_from_repo_git_tree_with_sha`) → hardcoded fallback; `substitute_template` does exact-brace `{{variable}}` substitution (no partial-name clobbering, no recursive expansion — both tested). `POST /repos/:id/prompts/save` commits to the default branch and returns the commit SHA; `validate_prompt_path` confines the MR-review bypass to `specs/prompts/` (absolute/`..`/root/prefix/backslash/non-UTF-8/outside-dir all rejected), mutation-verified.
- **Rate limiting**: in-memory sliding window (10 req/60 s keyed on auth-derived `(principal, workspace)`) applied inside each handler after auth, distinct from the global limiter; `ApiError::RateLimited` renders 429 + `Retry-After`; 60 s inactivity eviction via `spawn_llm_rate_limiter_cleanup` (wired in `main.rs`). Agent principals key on `agent:<agent_id>` when no user identity — documented and sound.
- **Model + token limits**: `resolve_llm_model` = config override → workspace `llm_model` → `GYRE_LLM_MODEL` → default; `max_tokens_for_function` maps GENERATE/ASK/ASSIST to 2000/4000/4000 with env overrides, kept pure for testability.
- **Budget charging**: every LLM call records both a `CostEntry` (`llm_query`) and a persisted `BudgetCallRecord` with prompt-template git SHA (new portable migration `2026-10-09-000056_budget_prompt_sha`, SQLite + Postgres adapters, no todo!/unimplemented! anywhere in the touched files) and increments workspace/tenant counters; unresolvable tenant scope is skipped-and-logged, not fabricated.
- **ABAC**: `RouteResourceMapping` entries for all four routes (explorer_view/generate, workspace/generate, spec/generate ×2); cross-workspace `repo_id` rejected via `validate_repo_scope`/`validate_repo_ownership` (hallucinated foreign repo_id in LLM view specs → null-view-spec fallback, not a leak); explorer generate additionally enforces in-handler workspace membership.

## Non-blocking observations (recorded, not findings)

- The handlers consume the full LLM stream before emitting SSE events, so `partial` events are chunked after the fact rather than truly incremental token streaming. The HTTP contract (Content-Type, partial/complete/error event semantics) is met and the task plan itself staged these as endpoint wiring with LLM integration refined later; noted for any future "true streaming" requirement.
- `explorer_views.rs` uses `let _ = state.costs.record(...)` (error silently dropped) where `specs_assist` logs a warning on the same call; the BudgetCallRecord path logs failures in all handlers. Cosmetic inconsistency only.

## Evidence

- `/tmp/stage/review-evidence/focused-tests-c6c9390d.log` — full focused suite output (239 passed / 0 failed).
- `/tmp/stage/review-evidence/focused-tests-summary.md` — command, exit code, key test inventory.
- `/tmp/stage/review-evidence/mutation-probes.md` — both mutation probes with outputs and revert confirmations; working tree clean after.
