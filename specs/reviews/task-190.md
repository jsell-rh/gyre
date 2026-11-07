# Review — task-190 (Record real per-call LLM usage into budget counters and audit log)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Budget Tracking — every LLM invocation appends a per-call usage record and rolls into the real-time workspace/tenant `tokens_used_today`/`cost_today` counters that `check_spawn_budget` evaluates.

Candidate `dd952573d996fb1327811108c3bb38e7b040938d` (base `4d6759a0b`); cumulative diff touches 16 files, +970/−25. Working tree verified clean at the candidate SHA before and after every probe (`git status --short` empty).

Verdict: **complete** (approved).

## Independent evidence

All commands and full output under `/tmp/stage/review-evidence/task-190-review.md`; all cargo runs `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo`.

- **SQLite adapter round-trip** (`cargo test -p gyre-adapters budget_call`): 3/3 pass — every field round-trips through a real INSERT/SELECT against `budget_call_records` (diesel mapping verified in `schema.rs:621-634`), workspace/since/limit filtering newest-first, duplicate PK rejected. Not a structural adapter.
- **Server budget tests** (`cargo test -p gyre-server --lib api::budget`): 9/9 pass, including the two new ones: `record_llm_call_usage_increments_workspace_and_tenant_counters` (workspace AND tenant counters at 1000 tokens/$0.05, one `BudgetCallRecord` retrievable) and `recorded_usage_makes_token_budget_limit_fire` (500/1000 → Ok, 1100/1000 → `Err` naming `max_tokens_per_day`).
- **Full HTTP path** (`cargo test -p gyre-server --lib agent_usage_report_records_budget_call`): 1/1 pass — POST `/api/v1/agents/:id/usage` (600+400 tokens, $0.12) appends exactly one `agent_run` record via `budget_calls.list_by_workspace`, increments workspace and tenant counters, and `GET /api/v1/workspaces/:id/budget` reflects 1000/$0.12.
- **Discrimination (kill) probes — both performed by this review on the candidate tree:**
  1. Removed the `record_llm_call_usage` call from `record_agent_usage` (spawn.rs) → the HTTP-path test FAILS (`usage report must append exactly one BudgetCallRecord: left 0, right 1`).
  2. Removed the `record_budget_usage` delegation inside `record_llm_call_usage` (budget.rs) → `recorded_usage_makes_token_budget_limit_fire` FAILS (`1100/1000 tokens used: spawn must be rejected`).
  Both mutations reverted from saved copies; tree re-verified pristine. The tests are wired to the production behavior, not to themselves.
- **Touched-module regressions** (`-- api::specs_assist api::explorer_views api::graph api::spawn api::budget api::mcp`): 105/105 pass; `mem::`: 13/13 pass.
- **Invariants:** `check-arch.sh` pass (port in gyre-ports, adapters in gyre-adapters, wiring in gyre-server); `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, `check-mcp-write-tools.sh` all pass.

## Verified in source

- **Single entry point, no parallel path.** `budget::record_llm_call_usage(state, &LlmCallUsage)` appends one `BudgetCallRecord` and delegates the counter increment to the pre-existing `record_budget_usage` — exactly the reuse the task mandated; no call site re-implements the increment. All five LLM invocation paths funnel through it: agent usage report (`spawn.rs`), REST specs/assist, MCP `spec_assist`, explorer-views/generate, briefing/ask.
- **Real values, no fabrication.** `record_agent_usage` uses the reported `tokens_input`/`tokens_output`/`cost_usd` verbatim; `model` falls back to the workspace's configured model then `DEFAULT_LLM_MODEL`. The llm_query paths keep the pre-existing estimate arithmetic unchanged for the analytics `CostEntry` (`(estimated_input + 500) * 3.0` — same value as before the refactor) and split the same estimate into input/output for the per-call record; no hardcoded token counts were introduced anywhere.
- **Scope handling is server-derived.** `record_agent_usage` loads the agent's workspace row and takes `tenant_id` from it (never caller-supplied); the llm_query paths resolve the workspace row by the path id behind pre-existing membership checks (`check_workspace_membership`, `require_workspace`) and take `tenant_id` from that row. Missing workspace → warn + skip (no fabricated `"default"` scope), consistent with the fabricated-scope-defaults invariant. `agent_id`/`task_id` are `None` on user-initiated queries, matching the spec's per-call struct comments.
- **Durable state is port-backed.** `AppState.budget_calls` wired via the `store!` macro (PG > SQLite > mem); the mem adapter enforces the duplicate-id contract in code, mirroring the SQL adapters' PK behavior (mem-port-contracts invariant satisfied).
- **Best-effort semantics are deliberate and correct**: a failed append logs a warning and never discards the already-produced LLM answer; the counters still increment via `record_budget_usage`, which ignores its own errors as before.

## Notes (non-blocking)

- `specs/coverage/system/platform-model.md` row 29 still reads "Hollow / task-190 not-started"; coverage-matrix reconciliation belongs to the spec-fidelity auditor process, not this task's acceptance criteria, and the shipped behavior is real per the probes above.
- `record_budget_usage` aggregates tenant usage under a single global `tenant_key()` rather than per-tenant keys. This is pre-existing behavior the task explicitly required reusing ("keep `record_budget_usage` as the single counter-increment entry point"); the per-call records do carry the true per-tenant `tenant_id`. Any move to per-tenant aggregate keys belongs to task-191 (enforcement) or a spec amendment, not this recording task.
- llm_query records carry `cost_usd: 0.0` because no cost model exists for estimates (the pre-existing analytics entries also carry no dollar cost); token counts are what the budget limits evaluate. Not fabrication — the unknown is recorded as zero rather than invented.
