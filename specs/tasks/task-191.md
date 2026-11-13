---
title: "Implement budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment"
spec_ref: "platform-model.md §Enforcement Behavior"
depends_on:
  - task-190
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Enforcement Behavior"
commits: ["1f28b18839adaa0092f89b9167688768a0d0de16", "c39c673ce4a88ab3ccd36cf3c5c0c45a310ce80c", "75adf7def12eeecb1e92e600e1b57deffcc9fe3c", "b3dea29003054abbca3c29b2a0ad83f586504d86", "2edf9f01c0a1dac4de43daa6fac6926cde8059a9"]
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

Budget enforcement is real end-to-end (platform-model.md §5 Enforcement
Behavior): the task-190 usage counters drive the specced actions on every
usage recording, via the single `check_and_enforce_budget` helper invoked
only from `record_llm_call_usage` (no per-call-site threshold logic).

This round repaired the CI Clippy failure from run 43331768394 at head
`9b14e0c8e`: the merge with base `44a01390` (task-171) added the
`prompt_template_sha` audit field to `LlmCallUsage`, but the task-191
enforcement test struct literals predated it, so the gyre-server test
targets no longer compiled (E0063 at `api/budget.rs:1449`/`:1533`).
Commit `2edf9f01c` passes `prompt_template_sha: None` in the two
enforcement fixtures — same repair class as `1f28b1883` in the prior
round. No production code changed this round.

### Behavior (unchanged from the verified rounds)

- `BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause,
  serde snake_case) in `gyre-domain/src/budget.rs`, re-exported;
  `BudgetConfig.enforcement_action: Option<...>` (None = spec default)
  persisted through migration `2026-10-10-000058`, the SQLite + Postgres
  row mappings, and the budget API DTOs (PUT/GET round-trip tested).
- Utilization = higher of `max_tokens_per_day` / `max_cost_per_day`
  (None/zero guarded). ≥80% <100%: `MessageKind::BudgetWarning` per
  active agent (workspace feed + Directed `Destination::Agent` copy for
  the MCP poll channel) and a spawner notification. ≥100%: default
  `GracefulStop` — `MessageKind::BudgetExhausted` per active agent, then
  an injectable grace timer (default 60s); still-Active agents are killed
  via the shared `kill_agent` helper (extracted from `admin_kill_agent`,
  clean cutover): Dead transition, process kill, budget counter
  decrement, worktree/branch preserved, reassignment task
  "Agent hit budget limit on {task-id}, needs reassignment or budget
  increase" created. `HardKill` kills inline; `QueuePause` pauses each
  workspace repo's merge queue (kv pause + `MergeQueuePaused` event)
  without killing. Emission is de-duplicated per daily period via a
  persisted kv marker keyed to `period_start`.

### Test evidence

- `cargo clippy -p gyre-server --all-targets --all-features -- -W
  clippy::all`: exit 0, zero warnings on PR-changed lines; the exact CI
  gate `python3 scripts/check-clippy-diff.py HEAD^1` passes ("changed
  lines clean").
- `cargo test -p gyre-domain`: 474 + 8 passed.
- `bash scripts/check-arch.sh`, `bash scripts/check-migration-versions.sh`:
  pass.
- gyre-server test compilation is blocked in this sandbox by the 4 GiB
  codegen budget (wrapper-refused; infrastructure restriction, not a code
  defect). The independent verifier must run: `cargo test -p gyre-server
  --lib api::budget` — covering `warn_at_80pct_emits_message_and_notification_no_kill`,
  `below_threshold_emits_neither_message_nor_notification`,
  `exhausted_graceful_stop_kills_preserves_work_creates_reassignment`,
  `exhausted_hardkill_override_kills_immediately_without_timer`,
  `exhausted_queuepause_override_pauses_workspace_merge_queues`,
  `exhausted_crossing_cost_limit_also_fires`,
  `budget_threshold_notifications_fire_once_per_period`,
  `budget_enforcement_action_serde_round_trips`,
  `enforcement_action_persists_through_budget_api`, and the task-190
  counter tests. These tests fail if enforcement is removed (assertions
  on emitted message/notification counts, Dead transition, reassignment
  task, and below-threshold silence).
