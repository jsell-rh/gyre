# Review — task-191 (Implement budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement
Behavior. Commits under review: `b3dea290` (implementation), `75adf7def`
(review-repair, agent-inbox delivery), merge `262782a6` (rebase onto assignment
base `fefde294`); docs/process commits touch only the task file and coverage
row. Candidate `3b30a3c6`, diff vs base: +1349/−57 across 13 files.

Verdict: **complete** (round 2 — finding 2441b169 verified fixed).

## Round 2 (this review)

Reviewer sandbox blocks crates.io (egress proxy: connect refused to
static.crates.io / index.crates.io, exit 7) and has no registry cache or
prebuilt target — cargo probes impossible. This is an infrastructure
restriction, not a code defect. Evidence and probe outputs:
`/tmp/stage/review-evidence/task-191-review.md`. Static verification was
maximized instead; remote verification must run the cargo commands listed at
the end.

### Prior finding 2441b169 (BudgetWarning/BudgetExhausted never reached the agent MCP inbox) — FIXED

`emit_budget_warn` / `emit_budget_exhausted` now emit both a workspace-feed
copy and a `Destination::Agent(agent.id)` copy per active agent with identical
payloads (the `constraint_check.rs:1315-1336` dual-destination pattern). All
three adapters' `list_after` filters `to_type == "agent"` and
`list_by_workspace` excludes it (sqlite/message.rs:193/324, mem.rs:3324/3411,
postgres mirrors), so the directed copies are exactly what the agent MCP poll
channel (`GET /api/v1/agents/:id/messages`, MCP `messages_list`) returns, and
workspace-feed counts are unaffected. The dispatch consumer is a no-op drain
(lib.rs:1085-1095) — no duplicate side effects. The three threshold tests now
pin agent-inbox receipt (`agent_inbox` helper over `messages.list_after`)
including no-duplicate assertions within the period.

### Verified working (no findings)

- **Enum + persistence**: `BudgetEnforcementAction` (Warn/GracefulStop/
  HardKill/QueuePause, serde snake_case) in `gyre-domain/src/budget.rs`,
  re-exported; `BudgetConfig.enforcement_action: Option<...>` (`None` = spec
  default) threaded through migration `2026-10-10-000058` (portable
  `ALTER TABLE ... ADD COLUMN ... TEXT`), SQLite + Postgres row mappings and
  upserts (absent field clears the column), and the budget API DTOs
  (`SetBudgetRequest`/`BudgetResponse`). PUT with an invalid action string
  fails deserialization (400), not a stored arbitrary string.
- **Single funnel**: `check_and_enforce_budget` has exactly one production
  caller — `record_llm_call_usage`, the task-190 funnel with five real call
  sites (agent usage report spawn.rs:1558, briefing/ask graph.rs:1464,
  explorer_views.rs:605, specs_assist.rs:459, MCP spec_assist mcp.rs:2696).
  No call-site threshold logic (grep: no other `record_budget_usage` callers).
- **Thresholds**: higher utilization across `max_tokens_per_day` /
  `max_cost_per_day`; None and zero limits guarded against divide-by-zero.
  ≥80%/<100% → Warn: BudgetWarning message per active agent (feed + directed)
  and one priority-7 `NotificationType::BudgetWarning` per unique spawner via
  `notifications::notify`. ≥100% → default GracefulStop: BudgetExhausted
  (dual destination), `tokio::spawn` grace timer (default 60s, injectable via
  `check_and_enforce_budget_with_grace`), then still-Active agents killed.
  Emission is driven solely by computed utilization — no timers or
  unconditional paths.
- **Kill path**: shared `kill_agent` extracted from `admin_kill_agent` (clean
  cutover — admin handler migrated, no fork); Dead transition, process kill
  with PID capture + attested-PID fallback, worktree cleanup skipped for
  budget kills (`preserve_worktree=true` — work preserved per spec), counter
  decrement only when the agent held an Active slot (completion/fail/stop
  paths transition status before decrementing, so no double-release), token
  revocation and `AgentKilled` audit with caller attribution. The rebase
  resolution folded the base's audit hardening INTO the helper; the base's
  pinned admin-kill assertions (reason "force-killed by admin", killed_by
  "system", revoke reason "admin_kill", single events) are preserved.
- **Reassignment task**: exact spec title "Agent hit budget limit on
  {task-id}, needs reassignment or budget increase", parent_task_id-linked,
  High priority, workspace derived from the agent (no fabricated scope),
  TaskCreated event + AgentEscalation notification to the spawner.
- **Overrides**: HardKill kills inline (no timer); QueuePause pauses every
  workspace repo's merge queue via the existing merge-processor mechanism
  (kv `merge_queue_pause` + `MergeQueuePaused` event) without killing; Warn
  at 100% warns only.
- **De-dup**: persisted kv marker (`budget_enforcement` ns) keyed to
  `period_start`; `reset_daily_counters` stamps a new period_start in all
  three adapters, so thresholds re-arm after the midnight reset and repeated
  tool calls never re-spam.
- **Tests are real**: assertions on observable state (repository drains,
  notification store, agent status, active-agent counter, worktree record
  presence, reassignment task existence) that fail if the enforcement helper
  or the directed emission is removed; below-threshold silence guards
  over-firing; no-re-fire assertions cover all three channels. Every API
  signature the tests use was verified against ports/domain definitions
  (compile-risk elimination for the uncompiled repair commit — see evidence
  file §3).
- **Mechanical gates**: check-arch, migration versions + portability, dead
  message kinds, mem port contracts, inert enforcement, task commit
  attribution, ABAC registry + exempt handlers, and the other security
  invariant scripts all PASS. rustc parse check on all six touched files:
  zero syntax-class errors.
- Migration version collision with the base's `2026-10-08-000058` avoided
  (date-prefixed Diesel versions are distinct; `check-migration-versions.sh`
  passes).

### Minor (not blocking, recorded for completeness)

- The grace timer is in-memory: a server restart during the 60s grace loses
  the kill while the exhausted marker blocks re-fire for the rest of the
  period. Spec is silent on crash-during-grace.
- With N active agents, N identical workspace-feed copies are emitted (one
  per agent) — cosmetic feed noise; per-agent delivery is the directed copy.
- Concurrent recordings racing the marker load/save can double-emit
  (best-effort dedup, rare).
- Workspace-level configurability only (tenant/repo levels unthreaded) —
  matches the task plan and is noted in the coverage row.

### Remote verification must run (egress-blocked here)

1. `cargo test -p gyre-server --lib api::budget` — the agent-inbox
   assertions added by `75adf7def` have never been compiled/run.
2. `cargo test -p gyre-server --lib admin` — kill-helper cutover.
3. `cargo build --all` + `bash scripts/check-arch.sh` on a networked host.

## Round 1 (prior review, checkpoint ed970cb)

Empirically ran the enforcement path end-to-end (85% → warning emitted,
agent inbox empty) and found the single defect 2441b169 —
BudgetWarning/BudgetExhausted emitted `Destination::Workspace` only, so the
agent MCP poll channel (filtering `to_type == "agent"`) never delivered them.
Fixed by `75adf7def` and verified structurally this round.
