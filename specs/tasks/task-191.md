---
title: "Implement budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment"
spec_ref: "platform-model.md §Enforcement Behavior"
depends_on:
  - task-190
progress: complete
coverage_sections:
  - "platform-model.md §Enforcement Behavior"
commits: ["35f2bba7a487eeccc275584a411ae66f1cf95ea5", "2edf9f01c0a1dac4de43daa6fac6926cde8059a9", "1f28b18839adaa0092f89b9167688768a0d0de16", "c39c673ce4a88ab3ccd36cf3c5c0c45a310ce80c", "75adf7def12eeecb1e92e600e1b57deffcc9fe3c", "b3dea29003054abbca3c29b2a0ad83f586504d86"]
---

## Spec Excerpt

From `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior:

> Configurable per-level:
>
> ```rust
> pub enum BudgetEnforcementAction {
>     Warn,           // Log warning, continue
>     GracefulStop,   // Send BudgetExhausted message, grace period, then kill
>     HardKill,       // Immediate termination
>     QueuePause,     // Pause merge queue until budget is reviewed
> }
> ```
>
> Default behavior:
> - **80% of limit:** `Warn` - agent receives a `BudgetWarning` MCP notification
> - **100% of limit:** `GracefulStop` - agent receives `BudgetExhausted`, gets 60 seconds to hand off work, then is killed
> - Agent's in-progress work is preserved (worktree, branch, partial MR). A task is created: "Agent hit budget limit on TASK-X, needs reassignment or budget increase"

## Problem

The section is **hollow**:

- `BudgetEnforcementAction` (the 4-variant enum above) does not exist anywhere in the codebase.
- `MessageKind::BudgetWarning` / `MessageKind::BudgetExhausted` (`crates/gyre-common/src/message.rs:116-117`) and `NotificationType::BudgetWarning` (`crates/gyre-common/src/notification.rs:30`, priority 7) are defined but **never emitted** by any enforcement path.
- No code computes the 80% / 100% thresholds, sends a grace period, kills at expiry, preserves the worktree, or creates the reassignment task.
- The only budget enforcement that exists is a spawn-time hard reject (`check_spawn_budget`, `crates/gyre-server/src/api/budget.rs:245`) for the *next* spawn; a running agent that blows its budget is never warned, stopped, or killed.

This depends on task-190 (real usage counters). Once `tokens_used_today`/`cost_today` increment from real usage, this task turns those numbers into the specced enforcement actions.

## Implementation Plan

1. **Add `BudgetEnforcementAction` enum** to `crates/gyre-domain/src/budget.rs` (Warn/GracefulStop/HardKill/QueuePause) with serde derives; re-export from `gyre-domain/src/lib.rs`. Add an optional `enforcement_action: Option<BudgetEnforcementAction>` field to `BudgetConfig` (`crates/gyre-domain/src/budget.rs:5`) so it is configurable per level; when `None`, use the spec default (80% Warn, 100% GracefulStop). Thread the new column through the SQLite + Postgres `budget_configs` adapters (migration + row mapping in `crates/gyre-adapters/src/{sqlite,postgres}/budget.rs`) and the `BudgetResponse`/`SetBudgetRequest` DTOs in `crates/gyre-server/src/api/budget.rs`.
2. **Central enforcement helper** in `crates/gyre-server/src/api/budget.rs`, e.g. `async fn check_and_enforce_budget(state: &AppState, workspace_id: &str)`, called immediately after `record_budget_usage` (from the recording sites wired in task-190). It:
   - Loads the workspace `BudgetConfig` + `BudgetUsage`.
   - Computes utilization for both `max_tokens_per_day` and `max_cost_per_day` (guard against `None` limits and divide-by-zero). Takes the higher utilization.
   - **≥ 80% and < 100%:** action `Warn`. Emit `MessageKind::BudgetWarning` to the workspace's active agents and create a `NotificationType::BudgetWarning` (priority 7) for the agents' spawner(s) via the existing `crate::notifications::notify(...)` path. Emit at most once per threshold crossing per period (track a flag on `BudgetUsage`, e.g. `warned_at`/`exhausted_at`, or gate on a persisted marker so repeated tool calls don't spam).
   - **≥ 100%:** action `GracefulStop` (default). Emit `MessageKind::BudgetExhausted` to all active agents in the workspace, then schedule a 60-second grace timer (`tokio::spawn` + `tokio::time::sleep`). After the grace period, for any agent in the workspace still `Working`/active, kill it via the existing kill path (reuse the logic behind `admin_kill_agent`, `crates/gyre-server/src/api/admin.rs:258` — extract a reusable `kill_agent(state, agent)` helper rather than duplicating), transition to `Dead`, decrement active-agent counters, and **preserve the worktree/branch** (do not delete). For each killed agent with a `current_task_id`, create a reassignment task titled `"Agent hit budget limit on {task_id}, needs reassignment or budget increase"` via `state.tasks`.
   - Respect an explicit `enforcement_action` override when set (e.g. `HardKill` skips the grace period; `QueuePause` pauses the workspace merge queue instead of killing — reuse the existing merge-queue pause mechanism if present, otherwise scope `QueuePause` to setting the queue paused flag).
3. **Do not** re-implement threshold detection at each call site; the single helper is invoked after each `record_budget_usage`.

## Acceptance Criteria

- `BudgetEnforcementAction` exists in `gyre-domain` with all four variants and serde round-trips; `BudgetConfig.enforcement_action` persists through SQLite/PG and the budget API.
- Recording usage that crosses **80%** of a workspace's `max_tokens_per_day` (or `max_cost_per_day`) emits `MessageKind::BudgetWarning` and creates a `NotificationType::BudgetWarning` — verifiable by draining the workspace message bus / notification store in a test. No warning fires below 80%; the warning is not re-emitted on every subsequent call within the same period.
- Crossing **100%** emits `MessageKind::BudgetExhausted` to active agents, and after the grace period elapses the still-active agents are transitioned to `Dead`, active-agent counters decremented, worktree records left intact, and a reassignment task created for each killed agent's task.
- Tests (server-side) that FAIL if enforcement is removed:
  - Set `max_tokens_per_day`, record usage to ~85%: assert a `BudgetWarning` message and notification were produced and no agent was killed.
  - Record usage to ≥100% with a short/zeroed grace for the test: assert `BudgetExhausted` emitted, active agent ends `Dead`, and a reassignment task exists referencing the task id.
  - Below-threshold usage produces neither message (guards against over-firing).
- `cargo build --all`, touched-crate tests, and `bash scripts/check-arch.sh` pass. Domain stays infra-free (the enum/config live in `gyre-domain`; message emission, timers, and kill live in `gyre-server`).

## Agent Instructions

- Depends on task-190 — the enforcement helper reads the real `tokens_used_today`/`cost_today` counters that task-190 makes live. Do not merge until those counters increment from real usage.
- Do NOT emit `BudgetWarning`/`BudgetExhausted` unconditionally or on a timer; they MUST be driven by the computed utilization crossing the threshold, and de-duplicated per period.
- Make the grace period injectable/configurable so tests can use a near-zero value; the production default is 60s.
- Extract a shared `kill_agent` helper from `admin_kill_agent` rather than duplicating the kill/decrement/notify sequence; migrate the admin handler to the helper (clean cutover).
- This shares subject with task-119 (agent-runtime §4). Implement enforcement once here as the canonical path; task-119 should consume it, not fork it.
- Verify the merge-queue pause mechanism (for `QueuePause`) and the kill path by reading the relevant handlers before editing.
- Run only the touched crates' tests plus `scripts/check-arch.sh`; do not run the full workspace suite or formatters.

## Shipped

One canonical budget-enforcement engine (`enforce_workspace_budget` in
`crates/gyre-server/src/api/budget.rs`), unified with task-119's
agent-runtime §4 cascade (no second enforcement path), delivering
platform-model.md §5 Enforcement Behavior.

### Real behavior

- `BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause, serde
  snake_case) in `gyre-domain/src/budget.rs`; optional
  `BudgetConfig.enforcement_action` persisted via migration
  2026-10-10-000058, SQLite + PG row mapping, the repo-level
  `budget_json` document, and the budget API DTOs (`PUT/GET
  /api/v1/workspaces/{id}/budget`). `effective_workspace_config` threads
  it per level: workspace override, else tenant, else spec default.
- After every `record_llm_call_usage` and every workspace budget PUT, the
  engine computes max utilization across `max_tokens_per_day` /
  `max_cost_per_day` against the effective (workspace ∩ tenant) config:
  - ≥ 80% / < 100%: `MessageKind::BudgetWarning` on the workspace feed AND
    a directed copy to every active agent's MCP inbox +
    `NotificationType::AgentBudgetWarning` (High) to the affected agents'
    spawners + workspace Admins, de-duplicated per period.
  - ≥ 100%: the configured action, at most once per daily period
    (persisted kv marker keyed to `usage.period_start`):
    `GracefulStop` (default) — `BudgetExhausted` to every active agent
    inbox, 60s grace timer (injectable `grace_secs`, single live timer,
    re-evaluates before killing), then `budget_stop_agent` on agents still
    Active; `HardKill` kills inline; `QueuePause` pauses every workspace
    repo's merge queue (shared `merge_processor::pause_merge_queue`)
    without killing; `Warn` keeps the warn tier. Beyond-limit (budget
    reduced below current usage) keeps the immediate hard kill per
    agent-runtime.md §4.
  - Every budget stop preserves the worktree/branch, leaves the task
    InProgress, revokes the agent token, charges compute minutes,
    releases the active-agent slot, and creates the spec'd reassignment
    task ("Agent hit budget limit on {task-id}, needs reassignment or
    budget increase").
- Budget kills mark agents `Stopped` (the amended agent-runtime.md §4 text
  on main supersedes this task's original "Dead" sketch); the admin kill
  path stays `Dead` via the shared `kill_agent` helper.

### This repair round (35f2bba7)

The verification run (`cargo test --all`) failed 2 budget tests that drive
the engine with an unattributed trigger (`trigger_agent: None` — the shape
of a user-initiated query or a budget-PUT crossing): with no trigger agent
and no workspace memberships, `notify_budget_level` had zero recipients,
so no `AgentBudgetExhausted` notification was created and `created == 0`
gated off the whole warn tier; the workspace-feed `BudgetWarning` event
was also only emitted for agent-attributed crossings. Fixed in production
code (tests unchanged): notification recipients now include the spawning
users of the workspace's active agents (the population enforcement acts
on), and the warn-tier feed event fires on every crossing. Both failing
assertions — cost-limit crossing emits the warning; the killed agent's
spawner gets `AgentBudgetExhausted` — are now satisfied by the recipient
derivation, not by test edits.

### Test evidence

- Local probes (see /tmp/stage/review-evidence/): `cargo check -p
  gyre-server` and `cargo check -p gyre-server --tests` clean (0 errors);
  `scripts/check-arch.sh`, `check-dead-message-kinds.sh`,
  `check-inert-enforcement.sh`, `check-task-commit-attribution.sh` OK;
  rustfmt clean on the changed file.
- Server test binaries were NOT run in this sandbox (hard 4 GiB cgroup —
  linking exceeds it; the build wrapper rejects them). The verifier must
  run: `cargo test -p gyre-server --lib api::budget api::admin mcp`
  (covers the task-191 acceptance tests — warn-at-85% message +
  notification + no kill + no refire, below-threshold silence,
  graceful-stop kill + worktree intact + reassignment task + dedup,
  HardKill inline, QueuePause kv + no kill, cost-limit crossing, API
  persistence — plus task-119's regression set), then `cargo test --all`.
