---
title: "Record real per-call LLM usage into budget counters and audit log"
spec_ref: "platform-model.md §Budget Tracking"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Budget Tracking"
commits: ["6b22fe6180b3a2e129e2de9ee4d1d6aedbddbfe0", "72bcef1b8698c6bd7197eda3546e55269c9b6097", "86fdbd014c6a2e3d0e966b8c12c3ec434c22961b", "ba3c8962c7adea547c5fffea732cce4e994ba6b1", "8aebeb62eb9de85dd2ee4f1665258bbcbbf8cc8e"]
---

## Spec Excerpt

From `specs/system/platform-model.md` §5 Resource Governance → Budget Tracking:

> Every MCP tool call that invokes an LLM records token usage:
>
> ```rust
> pub struct BudgetUsage {
>     pub tenant_id: Id,
>     pub workspace_id: Id,
>     pub repo_id: Option<Id>,       // None for user-initiated LLM queries (briefing/ask, explorer-views/generate)
>     pub agent_id: Option<Id>,      // None for user-initiated LLM queries
>     pub task_id: Option<Id>,
>     pub usage_type: String,        // "agent_run", "llm_query", etc.
>     pub input_tokens: u64,
>     pub output_tokens: u64,
>     pub cost_usd: f64,
>     pub model: String,
>     pub timestamp: u64,
> }
> ```
>
> The forge aggregates usage in real-time. Budget checks happen on every tool call, not just periodically.

(The per-call struct above is realized in code as `gyre_domain::BudgetCallRecord`, `crates/gyre-domain/src/budget.rs:26-43`. The real-time aggregate is `gyre_domain::BudgetUsage` with `tokens_used_today`/`cost_today`.)

## Problem

The section is **hollow — zero real usage recording**:

- `record_budget_usage()` (`crates/gyre-server/src/api/budget.rs:327`) — which increments the workspace + tenant `tokens_used_today` / `cost_today` counters — has **zero callers**. Grep confirms nothing invokes it. Consequently `check_spawn_budget` token/cost limits (budget.rs:265-280) can never fire: the counters are frozen at 0.
- `BudgetCallRecord` (`crates/gyre-domain/src/budget.rs:26`) is defined and the `budget_call_records` table exists (migration `2026-03-25-000024_platform_amendments/up.sql:16-34`, diesel `schema.rs`), but there is **no repository port, no adapter, and no code path that ever instantiates or persists a record**.
- Every LLM invocation site records only an analytics `CostEntry` with an *estimated* token count and no budget effect: `crates/gyre-server/src/mcp.rs:2215`, `crates/gyre-server/src/api/explorer_views.rs:519`, `crates/gyre-server/src/api/specs_assist.rs:415`. The agent usage report `POST /api/v1/agents/:id/usage` (`crates/gyre-server/src/api/spawn.rs:1329`) writes agent-row columns via `agents.record_usage()` but never touches the workspace/tenant budget counters.

Net effect: the forge does not aggregate LLM usage in real time, and per-day token/cost budgets are unenforceable because their inputs are always zero. This is the foundation for enforcement (§Enforcement Behavior) and must be real.

## Implementation Plan

1. **New port `BudgetCallRepository`** in `crates/gyre-ports/src/budget_call.rs` (add `pub mod budget_call;` + re-export to `crates/gyre-ports/src/lib.rs`):
   - `async fn save(&self, record: &BudgetCallRecord) -> Result<()>` — append-only insert.
   - `async fn list_by_workspace(&self, workspace_id: &str, since: u64, limit: i64) -> Result<Vec<BudgetCallRecord>>` — for later reporting/retention.
2. **Adapters** implementing the port, writing to / reading from `budget_call_records`:
   - SQLite: `crates/gyre-adapters/src/sqlite/budget_call.rs` (follow the pattern in `sqlite/budget.rs` / the existing usage adapters; use the diesel `budget_call_records` table already in `schema.rs`).
   - Postgres: `crates/gyre-adapters/src/postgres/budget_call.rs` (mirror SQLite).
   - In-memory: implement on the existing mem store in `crates/gyre-server/src/mem.rs` (a `Vec`/`Mutex` is fine — tests need real reads back).
   - Register modules in the adapters' `mod.rs` files.
3. **Wire into `AppState`** (`crates/gyre-server/src/lib.rs`): add `pub budget_calls: Arc<dyn BudgetCallRepository>` and construct it wherever the other budget repos are built (real SQLite/PG in server startup, mem in `mem::test_state`).
4. **Record on the agent usage path** (`spawn.rs:1329 record_agent_usage`): after `agents.record_usage(&usage)`, also:
   - Persist a `BudgetCallRecord { usage_type: "agent_run", tenant_id: agent.tenant_id, workspace_id: agent.workspace_id, repo_id: agent.repo_id, agent_id: Some(agent.id), task_id: agent.current_task_id, input_tokens, output_tokens, cost_usd, model, timestamp }` via `state.budget_calls.save(...)`.
   - Call `budget::record_budget_usage(&state, &agent.workspace_id.to_string(), tokens_input + tokens_output, cost_usd)` so the workspace + tenant `tokens_used_today`/`cost_today` counters actually increment.
5. **Record on the user-initiated LLM query paths** — at `mcp.rs:2215`, `explorer_views.rs:519`, `specs_assist.rs:415` (and any sibling `llm_query` recording site — grep `"llm_query"`): alongside the existing `CostEntry`, persist a `BudgetCallRecord { usage_type: "llm_query", repo_id: <repo ctx or None>, agent_id: None, ... }` and call `record_budget_usage` against the relevant workspace. Use the real token counts where the LLM response provides them; where only an estimate exists today, keep the estimate but split into input/output and record it consistently (do NOT invent a hardcoded value).
6. Keep `record_budget_usage` as the single counter-increment entry point; do not duplicate the increment logic at call sites.

## Acceptance Criteria

- A `BudgetCallRepository` port exists with SQLite, Postgres, and in-memory implementations; SQLite/PG persist to `budget_call_records` and reads return what was written.
- Reporting agent usage via `POST /api/v1/agents/:id/usage` (a) appends a `BudgetCallRecord` and (b) increments the workspace **and** tenant `tokens_used_today` and `cost_today`. A follow-up `GET /api/v1/workspaces/:id/budget` reflects the increased usage.
- Each user-initiated LLM query path (`specs/assist`, `explorer-views/generate`, briefing/ask) persists a `BudgetCallRecord` with `usage_type: "llm_query"` and increments the owning workspace counters.
- With a workspace `max_tokens_per_day` set and enough recorded usage, `check_spawn_budget` now returns the "max_tokens_per_day … exceeded" error — proving the counters are real inputs, not frozen zeros.
- Tests (in `spawn.rs` and/or `budget.rs` server tests) that FAIL if the wiring is removed:
  - After `record_agent_usage` with N tokens, `state.budget_usages.get_usage(workspace_key)` shows `tokens_used_today == N` and a `BudgetCallRecord` is retrievable via `budget_calls.list_by_workspace`.
  - After recording usage past `max_tokens_per_day`, `check_spawn_budget` returns `Err`.
- `cargo build --all`, touched-crate tests, and `bash scripts/check-arch.sh` pass (port lives in `gyre-ports`, adapters in `gyre-adapters`, wiring in `gyre-server` — no hexagonal violation).

## Agent Instructions

- Do NOT add a structural/no-op adapter. The SQLite and Postgres implementations MUST issue real INSERT/SELECT against `budget_call_records`; a test must read back a persisted record.
- Do NOT record a hardcoded token count to make the path fire. Use the real value from the usage report or the existing estimate, split into input/output.
- Reuse `record_budget_usage` for counter increments; do not reimplement increment logic inline.
- This task is the tracking foundation consumed by task-191 (enforcement) and shares subject with task-119 (agent-runtime §4). Build the recording helpers so both can reuse them; do not create a second parallel usage path.
- Verify the diesel `budget_call_records` table mapping in `crates/gyre-adapters/src/schema.rs` before writing the adapter.
- Run only the touched crates' tests plus `scripts/check-arch.sh`; do not run the full workspace suite or formatters.
