---
title: "Implement budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment"
spec_ref: "platform-model.md §Enforcement Behavior"
depends_on:
  - task-190
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Enforcement Behavior"
commits: ["75adf7def12eeecb1e92e600e1b57deffcc9fe3c", "b3dea29003054abbca3c29b2a0ad83f586504d86"]
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

Budget enforcement is now real end-to-end (platform-model.md §5 Enforcement
Behavior): the task-190 counters drive the specced actions on every usage
recording.

### Enum + persistence

`BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause, serde
snake_case) lives in `gyre-domain/src/budget.rs` and is re-exported.
`BudgetConfig.enforcement_action: Option<...>` (None = spec default: 80% Warn,
100% GracefulStop) is threaded through migration `000058` (portable
`ALTER TABLE ... ADD COLUMN enforcement_action TEXT`), the SQLite and Postgres
`budget_configs` row mappings, and the budget API (`SetBudgetRequest` /
`BudgetResponse`). Overwriting a config with the field absent clears the
column (upsert).

### Enforcement path

`check_and_enforce_budget` (single helper, invoked only from
`record_llm_call_usage` after `record_budget_usage` — no per-call-site
threshold logic):

- Computes the higher utilization across `max_tokens_per_day` /
  `max_cost_per_day` (None and zero limits guarded).
- **≥ 80%, < 100%:** emits `MessageKind::BudgetWarning` per active agent
  (both a workspace-feed copy and a **Directed `Destination::Agent`
  copy** so the agent's MCP poll channel — `messages.list_after`,
  `GET /api/v1/agents/:id/messages` — actually receives it) and one
  `NotificationType::BudgetWarning` (priority 7) per unique spawner via
  `notifications::notify`. No kill.
- **≥ 100%:** default `GracefulStop` — emits `MessageKind::BudgetExhausted`
  per active agent (workspace feed + Directed agent copy, so the agent
  learns to hand off within the grace), then a `tokio::spawn` grace timer
  (default 60s,
  injectable via `check_and_enforce_budget_with_grace` so tests use 0).
  After the grace, still-`Active` agents are killed through the shared
  `kill_agent` helper extracted from `admin_kill_agent` (admin handler
  migrated to it — clean cutover): transition to `Dead`, process kill,
  budget active-agent counter decremented (only when the agent held an
  `Active` slot — no double release), **worktree/branch records preserved**,
  and a reassignment task `"Agent hit budget limit on {task-id}, needs
  reassignment or budget increase"` created (`parent_task_id`-linked, High
  priority) plus an `AgentEscalation` notification to the spawner.
- Overrides: `HardKill` kills inline (no timer); `QueuePause` pauses every
  workspace repo's merge queue via the existing merge-processor pause
  mechanism (kv `merge_queue_pause` + `MergeQueuePaused` event) without
  killing; `Warn` at 100% warns only.
- Emission is driven solely by computed utilization and de-duplicated per
  daily period via a persisted kv marker (`budget_enforcement` namespace)
  keyed to `period_start` — after the midnight counter reset the thresholds
  can fire again; repeated tool calls never re-spam.

### Test evidence

`cargo test -p gyre-server --lib api::budget` — 17 pass,
including the acceptance tests that fail if enforcement is removed: 85%
warning (message + notification, agent alive, no re-fire on subsequent
calls, agent-inbox receipt pinned via `messages.list_after`),
below-threshold silence, ≥100% graceful stop (agent `Dead`,
counter 0, worktree record intact, reassignment task referencing the task
id, no re-fire), HardKill inline (no timer), QueuePause (kv pause state
set, agent alive), cost-limit crossing, serde round-trips, and
API/SQLite persistence. Post-merge with base cd4c04fe (merge commit
`bf2c1a998`, evidence in
`/tmp/stage/review-evidence/task-191-merge-cd4c04fe-evidence.md`):
admin kill tests 36 pass (35 prior + 1 new upstream, including
`admin_kill_emits_lifecycle_audit_events` pinning token revocation with
reason `admin_kill` and the `AgentKilled` audit attribution),
`signal_chain` 19 and `secret_rotation` 8 pass (kill path now also
revokes task-scoped secrets), `cargo check` across gyre-server /
gyre-adapters / gyre-cli / gyre-domain --all-targets is clean,
and `check-arch.sh`, the migration-version and migration-portability
checks, and the other relevant mechanical check scripts pass. The
full-suite runs (server lib 1492) and clippy cited below predate this
merge; full workspace gates are owned by verification.

### Review repair (finding 2441b169079c4baab45e073159c02136)

Budget messages were emitted `Destination::Workspace` only, so the agent
MCP poll channel (which filters `to_type == "agent"`) never delivered
them — the agent had no way to learn it should hand off within the grace
period. `emit_budget_warn` / `emit_budget_exhausted` now emit both a
workspace-feed copy and a `Destination::Agent(agent.id)` copy per active
agent (the dual-destination pattern already used by
`constraint_check.rs`), with identical payloads. Thresholds, per-period
de-dup, grace timer, kill path, and spawner notifications are unchanged;
the dispatch consumer is a no-op drain and WS delivery already ignores
agent-directed messages, so the extra copies have no side effects beyond
the agent inbox.

### Rebase resolution (merges of bases fefde294 and cd4c04fe)

Two assignment-base merges were resolved by folding upstream
`admin_kill_agent` additions INTO the shared `kill_agent` helper (no
fork of the kill/decrement/notify sequence):

- **fefde294:** audit hardening (auth extractor for `killed_by`
  attribution, PID capture + attested-PID fallback for the `AgentKilled`
  event, `revoke_agent_token`, `emit_killed`). The admin handler passes
  the caller identity and `"admin_kill"` (upstream test asserts this
  exactly); budget enforcement passes `"budget_enforcement"` /
  `"budget_kill"`.
- **cd4c04fe** (merge commit `bf2c1a998`, evidence in
  `/tmp/stage/review-evidence/task-191-merge-cd4c04fe-evidence.md`):
  (1) `emit_telemetry` became `async` upstream — the helper now `.await`s
  it, keeping the parameterized `reason` description; (2) upstream added
  `revoke_agent_task_secrets` (Platform Model §7 teardown) inline in the
  admin handler — folded into the helper after `revoke_agent_token`, so
  admin AND budget kills revoke task-scoped ephemeral secrets, matching
  that function's "every path that ends an agent run" contract. Two
  `BudgetConfig` test initializers in the upstream `signal_chain.rs`
  gained `enforcement_action: None` (spec default — no semantics change).

The budget migration `2026-10-10-000058_budget_enforcement_action` does
not collide with other `000058`-suffixed migrations (Diesel versions
include the date prefix; `check-migration-versions.sh` passes).

### Notes for review

- `record_llm_call_usage` now takes `&Arc<AppState>` (needed to hand the
  grace timer a `'static` state); all five call sites already passed `&state`
  from an `Arc`, and `mcp.rs handle_spec_assist` was widened to `&Arc` — no
  behavior change.
- Enforcement is workspace-level (per the task plan); tenant/repo-level
  configurability remains unthreaded (repo-level `budget` field is absent —
  coverage row 5 note).
- Shared subject with task-119 (agent-runtime §4): this is the canonical
  enforcement path; task-119 should consume `check_and_enforce_budget`.
