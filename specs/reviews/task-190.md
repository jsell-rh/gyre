# task-190 review — Record real per-call LLM usage into budget counters and audit log

Spec: platform-model.md §5 Resource Governance → Budget Tracking.
Candidate: `3f445ee5f1858f70e85cb2c4452a1d2ac049c268` (product commit
`f4440a1e`, plus merge `94122d74` reconciling with base `8d0625df` and two
process commits touching only this task file).
Full probe commands, sources, exit codes and outputs:
`/tmp/stage/review-evidence/task-190-review-evidence.md`.

## What was independently verified

- **Port + adapters are real.** `BudgetCallRepository` (gyre-ports) with
  SQLite and Postgres diesel adapters issuing real INSERT/SELECT against the
  pre-existing `budget_call_records` table (schema verified against
  migration 2026-03-25-000024), and a mem adapter enforcing the same
  duplicate-id contract in code. SQLite round-trip tested against a real
  temp-file DB (3 tests: field round-trip, workspace/since/limit filtering
  newest-first, duplicate rejection). Wired into `AppState.budget_calls`
  via the `store!` macro.
- **Single entry point.** `budget::record_llm_call_usage` appends one
  `BudgetCallRecord` then delegates the workspace+tenant counter increment
  to the pre-existing `record_budget_usage` — no duplicated increment logic;
  previously-dead `record_budget_usage` now has real callers.
- **All enumerated call sites wired with real values.** Agent usage report
  (`POST /api/v1/agents/:id/usage`, usage_type `agent_run`, real reported
  tokens/cost, scope derived from the agent's workspace row — never
  caller-supplied) and the user-initiated `llm_query` paths: REST specs/assist,
  MCP spec_assist (the only MCP tool that invokes an LLM — verified by grep),
  explorer-views/generate, briefing/ask. The estimate split preserves the
  pre-existing analytics estimate arithmetic exactly; no hardcoded token
  counts; nothing invented for cost (0.0 where the LlmPort provides no cost).
- **Tests discriminate (mutation probes, both restored and re-verified
  green).** Disabling the `record_llm_call_usage` call in `record_agent_usage`
  makes `agent_usage_report_records_budget_call_and_increments_counters`
  FAIL (0 records appended); disabling the `record_budget_usage` increment
  makes `recorded_usage_makes_token_budget_limit_fire` FAIL (1100/1000
  tokens no longer rejected). The acceptance-criterion behaviors — record
  appended, workspace AND tenant counters incremented, GET
  /workspaces/:id/budget reflecting usage, `check_spawn_budget` firing on
  `max_tokens_per_day` — are pinned by tests that fail with the wiring
  removed.
- **Focused suites pass on the candidate:** gyre-adapters `budget_call` 3/3;
  gyre-server api::budget 9/9, api::spawn 35/35, api::specs_assist 21/21,
  api::explorer_views 16/16 (includes main's merged grammar-validation
  tests), api::graph 23/23, mcp 79/79; gyre-ports builds (trait crate, no
  tests). `check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-in-memory-state-stores.sh`, `check-mem-port-contracts.sh` all OK.
  No new routes, migrations, or exemption entries.
- **Merge reconciliation spot-checked** against the shipped notes: the
  explorer_views.rs conflict genuinely combines both sides (budget recording
  plus main's LLM-output validation), and commit attribution holds (product
  commit listed in frontmatter; process commits touch only the task file).

## Non-blocking observations (for the coverage auditor / follow-up tasks)

- Two LLM invocation paths outside the task's enumerated scope still record
  no usage: `graph-predict` (REST) and the explorer WebSocket native-LLM
  session. Neither recorded an `llm_query` CostEntry before this task (the
  task's scoping grep), and the spec sentence under this section concerns
  MCP tool calls (only spec_assist, wired). Follow-up task material, not a
  defect against this task's contract.
- `cost_usd` is recorded as 0.0 on the estimate-only paths (LlmPort returns
  no cost data) — correct per the "do not invent values" instruction.
- Workspace lookups on llm_query paths are best-effort (`.ok().flatten()` →
  warn + skip), matching the pre-existing counter-increment posture.

## Verdict

The task contract is satisfied with real, production implementations and
tests that fail when the wiring is removed. Approved.

— Reviewer, independent review of candidate `3f445ee5`
