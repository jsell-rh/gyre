# Review — task-191 (Budget enforcement actions: warn / graceful-stop with grace, kill, reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior.
Comparison base: `fefde2946f3a0af1f212588f54591516fee0f7b2` → candidate `44543ad5ac8011ee582be84d3565851dc7d8807f` (branch `pipeline/task-191/2441b169079c4baab45e073159c02136-1`, includes merge of the assignment base in `262782a68`).
Diff: 13 files — domain enum/config, migration `2026-10-10-000058_budget_enforcement_action`, SQLite/PG budget adapters, `api/budget.rs` enforcement helper + tests, `api/admin.rs` `kill_agent` extraction, `mcp.rs` signature widening, docs, coverage row 28, task file. Working tree clean at candidate before and after every probe.

Verdict: **needs revision** (one code finding). All probe commands, sources, and outputs in `/tmp/stage/review-evidence/task-191-review.md`.

## What was verified as real

- **Enum + persistence**: `BudgetEnforcementAction` (4 variants, serde snake_case) in `gyre-domain/src/budget.rs`; `BudgetConfig.enforcement_action` threaded through migration 000058 (portable plain `ALTER TABLE`), SQLite + PG row mappings (upsert clears on overwrite), and the budget API DTOs. SQLite round-trip test passes (`gyre-adapters` budget: 4 passed).
- **Enforcement path**: single helper `check_and_enforce_budget`, invoked only from `record_llm_call_usage` (the task-190 funnel fed by `POST /api/v1/agents/:id/usage` and the four user-LLM paths). Utilization = max across token/cost limits, None/zero guarded; ≥80%<100% warn; ≥100% configured action defaulting to GracefulStop (60s injectable grace timer), HardKill inline, QueuePause via the real merge-processor pause mechanism (kv `merge_queue_pause`), Warn-at-100% warns only. Per-period de-dup via persisted kv marker keyed to `period_start`.
- **Kill path**: shared `kill_agent` extracted from `admin_kill_agent` (clean cutover — admin handler now delegates, passing caller identity and `admin_kill`/`admin_kill` revoke reason, preserving the base's audit hardening: `AgentKilled` event, `revoke_agent_token`, `emit_killed`, attested-PID fallback). Budget kills preserve worktree/branch records, decrement the active-agent slot only when held, and create the spec-titled reassignment task (`parent_task_id`-linked, High priority).
- **Tests are hard, not self-confirming** — proven by three mutations, each restored byte-identical:
  - enforcement call removed from `record_llm_call_usage` → warn test FAILS (0 messages);
  - `kill_exhausted_agents` disabled → graceful-stop AND hard-kill tests FAIL (Active ≠ Dead);
  - worktree preservation disabled → graceful-stop test FAILS (worktree record 0 ≠ 1).
- **Suites**: `api::budget` 17 passed, `api::admin` 35 passed, `gyre-server` lib 1492 passed, `gyre-adapters` budget 4 passed; `check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-dead-message-kinds.sh`, `check-unwired-port-methods.sh` all pass.

## Finding (blocking) — agents never receive the budget messages

Spec: "80% of limit: **Warn** - agent receives a `BudgetWarning` MCP notification"; "100%: **GracefulStop** - agent receives `BudgetExhausted`, gets 60 seconds to hand off work, then is killed".

`emit_budget_warn` / `emit_budget_exhausted` (api/budget.rs) emit only `Destination::Workspace(ws_id)` — one message per agent, all addressed to the workspace. Delivery surfaces:

- the agent MCP notification channel (`gyre_message_poll` → `GET /api/v1/agents/:id/messages` → `messages.list_after`) filters `to_type == "agent"` (sqlite/message.rs:210-211; mem.rs:3335) — workspace-destination messages never enter an agent inbox;
- WS delivery (ws.rs:400-412) sends `Destination::Workspace` messages to subscribed human clients and explicitly excludes agent-directed ones.

Empirical probe (review-evidence, probe 6): ran enforcement to 85%, then listed the agent inbox via `messages.list_after` — empty. The BudgetWarning/BudgetExhausted messages reach only the workspace feed, so the agent never learns it is at 80% or exhausted; the 60-second hand-off grace is inert from the agent's perspective. The in-repo precedent for the same Event tier (`ConstraintViolation`, constraint_check.rs:1323-1336) emits both a workspace copy and a directed `Destination::Agent(author)` copy — the budget path implements only the workspace half. The shipped tests count workspace-feed messages (`list_by_workspace`) only, so this acceptance gap is unpinned.

Fix direction: additionally emit `Destination::Agent(agent.id)` per active agent (the per-agent loop already exists), and extend the acceptance tests to assert the agent inbox (`list_after`) contains the message.

## Minor observations (not blocking)

- Grace timer is in-memory; a restart during the 60s grace orphans the kill and the persisted `exhausted` marker then suppresses re-firing. This matches the task plan's prescribed design (`tokio::spawn` + `sleep`); `check_spawn_budget` still gates new spawns. For the PM to weigh.
- `load_enforcement_marker` → emit → save spans awaits, so concurrent recordings could theoretically double-emit; the sequential no-re-fire acceptance criterion is enforced.
- Reassignment task uses `repo_id = Id::new("")` when the killed agent has no repo (Task.repo_id is non-optional per M34); the parent task's repo_id would be a better source. Edge case.
- Shared subject with task-119 (agent-runtime §4): the task notes correctly designate `check_and_enforce_budget` as the canonical path for task-119 to consume.

## Coverage section

Row 28 (Enforcement Behavior) should stay open (or revert to task-assigned/needs-revision) until the agent-directed delivery above lands; the workspace-level machinery (enum, persistence, thresholds, actions, kill, reassignment, de-dup) is real and mutation-proven.
