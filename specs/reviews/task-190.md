# Review — task-190 (Record real per-call LLM usage into budget counters and audit log)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Budget Tracking ("Every MCP tool call that invokes an LLM records token usage … The forge aggregates usage in real-time").
Candidate under review: `9295fcbf99eb2a90922686ad8e058a8ed58349f6` (merge of main `9bef21a4` into the task branch; implementation source `f4440a1e` recorded in the `commits:` frontmatter).
Verdict: **complete** (approved; Rust build/test execution deferred to verification — see transport note).

## Round 1 (this review)

### Transport note — probes blocked by sandbox, not by the code

crates.io is unreachable from this sandbox (curl to static.crates.io → exit 7,
connection refused; `cargo check --offline` → exit 101, no vendored registry
anywhere on the box). The interrupted prior review attempt of this exact
candidate failed on the same restriction. Per assignment rules this is
infrastructure evidence, not a code defect. All commands the verifier must run
are recorded in `/tmp/stage/review-evidence/task-190-review.md`; the review
below is a full static trace of the diff plus every non-cargo mechanical
invariant check.

### What was verified (diff `9bef21a4..9295fcbf`, 16 files, +972/−25)

- **Port real.** `BudgetCallRepository` (`gyre-ports/src/budget_call.rs`):
  append-only `save` (duplicate id is an error) + `list_by_workspace(ws, since,
  limit)` newest-first; re-exported in ports lib.rs.
- **Adapters real, not structural.** SQLite (`gyre-adapters/src/sqlite/
  budget_call.rs`): genuine diesel INSERT/SELECT against the pre-existing
  `budget_call_records` table; row mapping checked field-by-field against
  `schema.rs:621-634` and migration `2026-03-25-000024_platform_amendments/
  up.sql:16-34` (id PRIMARY KEY → duplicate insert fails in the DB, so the
  port's failure constraint is adapter-enforced); reads tenant-scoped via
  `self.tenant_id` like the sibling budget adapters. Postgres mirrors it
  (`postgres/budget_call.rs`, `check_for_backend(diesel::pg::Pg)`, shared
  `crate::schema`). Mem adapter (`mem.rs MemBudgetCallRepository`) enforces the
  duplicate-id contract in code — `check-mem-port-contracts.sh` passes.
- **Wiring complete.** `AppState.budget_calls` added via the `store!` macro
  (PG > SQLite > mem); all three AppState literal construction sites
  (build_state, mem::test_state_inner, middleware test clone) updated —
  grep-verified exhaustive.
- **Single entry point, no duplicated increment logic.**
  `budget::record_llm_call_usage` appends one `BudgetCallRecord` then delegates
  the counter increment to the pre-existing `record_budget_usage` (workspace +
  tenant `tokens_used_today`/`cost_today`). Grep confirms no call site touches
  `budget_usages` directly; `record_budget_usage` now has real callers, so the
  previously-dead-method flag no longer fires (`check-unwired-port-methods.sh`
  passes).
- **Agent usage path** (`spawn.rs record_agent_usage`, POST
  /api/v1/agents/:id/usage): after `agents.record_usage`, records
  `usage_type: "agent_run"` with the REAL reported tokens/cost; scope is
  server-derived (tenant from the agent's workspace row; workspace/repo/agent/
  task from the agent row) — never caller-supplied
  (`check-forged-scope-fields.sh` passes).
- **Every `llm_query` site wired**, scope resolved from the row the handler
  already loaded, `agent_id`/`task_id` None: specs/assist REST, MCP
  `spec_assist` (the only MCP tool that invokes an LLM — grep-verified — so the
  spec sentence "Every MCP tool call that invokes an LLM records token usage"
  is fully covered), explorer-views/generate, briefing/ask (which also gains
  the previously missing analytics CostEntry). The estimate split is honest:
  the analytics CostEntry amount is numerically unchanged, input+output equals
  the estimate, and no hardcoded token counts were introduced (cost_usd stays
  0.0 on estimate-only paths because LlmPort returns no cost — not an invented
  value).
- **Tests are discriminating by construction.** api::budget:
  counter-increment test (ws AND tenant) and the token-limit test
  (500/1000 → Ok; 1100/1000 → Err naming `max_tokens_per_day`); api::spawn:
  full-HTTP-path test (POST 600+400/$0.12 → exactly one `agent_run` record via
  `list_by_workspace`, ws AND tenant counters at 1000/$0.12, GET
  /workspaces/:id/budget reflects it). Logic trace: removing the
  `record_llm_call_usage` call or the `record_budget_usage` delegation freezes
  the counters at 0 and fails these assertions — the exact pre-task-190 hollow
  state. Execution deferred (see transport note); mutation-probe commands
  recorded for the verifier.
- **Merge reconciliation sound.** explorer_views keeps this task's budget
  recording AND main's grammar + repo-ownership validation (budget first,
  validation before `event: complete`); main's three LLM-output tests are
  present (:1181/:1215/:1241). The removed `use std::sync::Arc;` in spawn.rs
  was a true duplicate inside the single `mod tests` (merge artifact);
  remaining Arc usages are covered by the import at :1884.
- **No out-of-scope edits**: diff touches only the port/adapters/mem/server
  files above + `specs/tasks/task-190.md` bookkeeping. No exemptions added, no
  gates weakened.
- **Mechanical checks all pass at the candidate** (exit 0): check-arch,
  check-mcp-write-tools, check-unwired-port-methods,
  check-in-memory-state-stores, check-mem-port-contracts,
  check-abac-route-registry, check-migration-versions,
  check-task-commit-attribution (the task-210 failure noted in the shipped
  summary no longer reproduces at this later base), plus scope/secret/
  truncation/inert-enforcement/dead-message-kinds/ref-resolution/forwarded-
  header/unbounded-http families.

Findings:

- (none blocking)

Observations recorded for follow-up (outside this task's enumerated scope,
pre-existing at base): the explorer WebSocket native-LLM path
(`explorer_ws.rs:2552`) and `graph.rs predict_graph` (:1570) are user-initiated
LLM paths that record neither a CostEntry nor a BudgetCallRecord; they were
never `llm_query` recording sites and are not MCP tool calls, so they fall
outside both the task's sibling rule (grep `"llm_query"`) and the spec
sentence's MCP scope — but they should eventually be recorded for §Budget
Tracking's blanket intent. Also, the tenant aggregate intentionally remains
the pre-existing global `tenant:global` key (the record itself carries the
real per-workspace tenant_id) — `record_budget_usage` reuse was mandated by
the task plan.

— Reviewer, 2026-11-06 (candidate 9295fcbf; evidence in
/tmp/stage/review-evidence/task-190-review.md)
