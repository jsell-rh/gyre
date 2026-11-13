# Review — task-191 (budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior.
Base `44a01390` → candidate `ddd99bd4` (product commit `b3dea290` + inbox-delivery `75adf7de`
+ merge/CI-repair commits `2edf9f01c` etc.; the rest are docs/process).
Verdict: **approved** — implementation is real end-to-end; no blocking findings. Full probe
evidence: `/tmp/stage/review-evidence/` (summary.md + logs).

## What was verified (independently, review sandbox)

- **Enum + persistence.** `BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause,
  serde snake_case) in `gyre-domain/src/budget.rs`, re-exported; `BudgetConfig.enforcement_action`
  threaded through migration `2026-10-10-000058` (unique version `20261010000058`, portable
  plain `ALTER TABLE`), SQLite + Postgres row mappings, diesel schema, and the budget API
  DTOs. Executed probe: `cargo test -p gyre-adapters enforcement_action_round_trips_through_sqlite`
  → ok (all four variants + None-clearing upsert round-trip through real Diesel/SQLite).
- **Single enforcement funnel.** `check_and_enforce_budget` runs only from
  `record_llm_call_usage` (api/budget.rs:430) — the one usage entry point task-190 wired
  (agent usage report + all llm_query paths). No call-site threshold logic anywhere;
  `record_budget_usage` has no other production callers.
- **Thresholds are computed, not declared.** `budget_utilization` takes the max of
  token/cost utilization with None/zero-limit guards; ≥80%<100% → Warn, ≥100% → configured
  action (default GracefulStop). Emission is driven solely by the recorded counters — no
  timers or unconditional paths.
- **Real message delivery.** `MessageKind::BudgetWarning`/`BudgetExhausted` are emitted per
  active agent as a workspace-feed copy plus a Directed `Destination::Agent` copy, because
  the agent MCP poll channel (`list_after`) only returns agent-addressed messages — the
  directed copy is what makes the grace period meaningful to the agent. `emit_event`
  persists both through the messages port (verified in lib.rs:604-652). Payloads match the
  pre-registered kind schemas (message.rs:469-481) exactly.
- **Real kill path, shared helper.** `kill_agent` extracted from `admin_kill_agent`
  (admin.rs) with a clean cutover — the admin handler now calls the helper; budget kills
  pass `preserve_worktree=true` so worktree/branch records survive (kill only skips
  deletion; verified against the base handler diff). Dead transition, process kill, token
  revocation, task-secret revocation, audits all preserved. Active-agent slot decrement is
  guarded by `was_active` (only Active agents hold a slot), fixing the pre-existing
  admin-kill slot leak without double-decrement risk.
- **Reassignment task.** Created per killed agent's `current_task_id` with the spec's exact
  title "Agent hit budget limit on {task-id}, needs reassignment or budget increase",
  parent_task_id link, High priority, persisted via `state.tasks`.
- **Overrides.** HardKill kills inline (no timer); QueuePause pauses every workspace repo's
  merge queue via the real `merge_processor::pause_merge_queue` (kv `merge_queue_pause` +
  MergeQueuePaused event) without killing; Warn at 100% warns only.
- **De-dup.** Persisted kv marker (`budget_enforcement` ns) keyed to `usage.period_start`;
  `reset_daily_counters` re-stamps period_start at midnight so markers re-arm. Warn is also
  suppressed once exhausted has fired.
- **Tests fail if enforcement is removed.** Every assertion reads stores (messages,
  notifications, agents, worktrees, tasks, budget_usages, kv) that only the enforcement
  code writes: warn-at-85% (message + notification + no kill + no re-fire), below-threshold
  (neither), graceful-stop (exhausted emitted pre-grace, agent Dead, counter 0, worktree
  intact, reassignment task present, no re-fire), HardKill inline, QueuePause kv state +
  no kill, cost-limit crossing, API round-trip, serde round-trips. No mirrored logic,
  stubs, or conditional guards found.
- **Invariants.** `check-arch.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-dead-message-kinds.sh`,
  `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`
  all pass. Domain stays infra-free.
- **Compile/Clippy evidence.** `cargo check` (workspace) and `cargo check -p gyre-server
  --tests` pass — the latter is the exact failure class of CI run 43331768394 (E0063,
  missing `prompt_template_sha` in the enforcement test fixtures), repaired by `2edf9f01c`.
  `cargo clippy -p gyre-server --tests` exits 0 with zero warnings in any file this
  candidate touched.

## Deferred to the verifier (gyre-server test execution is wrapper-blocked in the review
sandbox — memory budget, not a code defect)

`cargo test -p gyre-server --lib api::budget` (all eight enforcement tests listed in
/tmp/stage/review-evidence/summary.md), plus `cargo build --all` / workspace suite / CI
Clippy per the task acceptance criteria.

## Non-blocking observations (recorded, no repair required)

1. Check-then-set race on the de-dup marker under truly concurrent recordings of the same
   crossing — worst case one duplicate message/notification; kill path is Dead-guarded.
2. Grace timer is in-process: a restart during the 60s grace loses the pending kill and the
   persisted marker suppresses re-fire for the rest of the period. Spec is silent on
   restart semantics.
3. N active agents → N workspace-feed copies per crossing (one per agent, agent_id-tagged),
   by design for per-agent attribution.
4. Per-level configurability is workspace-level only; already recorded as a remaining
   divergence in coverage row 28 (repo-level budget field absent, row 5).
