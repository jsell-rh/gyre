# Review — task-191 (Budget enforcement actions: warn / graceful-stop with grace, kill, reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior.
Candidate: `7f12262ed8db787f532e5312a4ec01260a9cb41a` (branch `pipeline/task-191/10e8bf2ae9584f0d8146c62ef69986a8-1`, HEAD of branch). Product commits under review: `b3dea290`, `75adf7de`, `c39c673ce` (+ merge `e038b72eb` onto base `7e9b3cfee`, process commits recorded in frontmatter).
Verdict: **approved** — the enforcement path is real, driven by the task-190 counters through a single funnel, and guarded by genuine tests. Server test *execution* is deferred to the independent verifier (wrapper-blocked here); exact commands recorded below.

## Probes run (independent, this review)

Evidence under `/tmp/stage/review-evidence/`:

- `cargo check -p gyre-server --lib` → PASS (7m, cold; includes web asset build).
- `cargo check -p gyre-server --tests` → PASS (3m23s) — the new test module type-checks, so the acceptance tests are not vaporware even though the wrapper blocks linking the test binary here.
- `cargo test -p gyre-adapters --lib budget` → **PASS 4/4**, including `sqlite::budget::tests::enforcement_action_round_trips_through_sqlite` (7m07s cold compile).
- `cargo test -p gyre-server --lib api::budget::tests::` → **exit 64, blocked by the build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB. … Server builds/tests belong to the independent verifier"). Infrastructure restriction, not a code defect.
- Mechanical invariant scripts, all PASS: `check-arch`, `check-migration-versions`, `check-migration-sql-portability`, `check-dead-message-kinds` (confirms `MessageKind::BudgetWarning`/`BudgetExhausted` now have emitters), `check-in-memory-state-stores`, `check-inert-enforcement`, `check-fail-open-ref-resolution`, `check-lossy-secret-conversion`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-mem-port-contracts`.
- Full diff read base→candidate for all 14 touched files; source cross-reads of `merge_processor::pause_merge_queue`, `MessageKind::as_str`, `AgentStatus`, messages port semantics, and `maybe_notify_budget_threshold`.

## Verified against the task contract

- **Enum + persistence.** `BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause, serde snake_case) in `gyre-domain/src/budget.rs`, re-exported from the domain lib. `BudgetConfig.enforcement_action: Option<_>` (None = spec default) threaded through migration `2026-10-10-000058_budget_enforcement_action` (plain portable `ALTER TABLE ADD COLUMN`; portability script passes; version distinct from base's `2026-11-08-000058` — duplicate-version script passes), `schema.rs`, and **symmetric** SQLite + Postgres row mappings (`enforcement_action_to_db`/`from_db` mirrored in both adapters). `SetBudgetRequest` carries the field; PUT/GET round-trip tested at the API level and through SQLite storage.
- **Single funnel, no per-site threshold logic.** `check_and_enforce_budget` is invoked exactly once, inside `record_llm_call_usage` right after `record_budget_usage`; every production recording site (spawn.rs agent-usage endpoint, mcp.rs spec_assist, explorer_views, specs_assist ×2, graph.rs) routes through `record_llm_call_usage`. Grep confirms no other production caller of `record_budget_usage`.
- **Threshold computation.** `budget_utilization` takes the max of token and cost utilization, guarding `None` and zero limits (no divide-by-zero). Warn at ≥80% <100%; exhausted at ≥100%; utilization drives emission — nothing fires on a timer or unconditionally.
- **Warn tier.** `MessageKind::BudgetWarning` emitted per active agent as BOTH a workspace-feed copy and a directed `Destination::Agent` copy — I verified the agent MCP poll channel (`messages.list_after`) only returns `to_type == "agent"`, so the directed copy is what actually reaches the agent; without it the emission would be theater. Spawner/Admin inbox notification routes through the pre-existing single path `maybe_notify_budget_threshold` (per-user per-period dedup via `has_active_notification`), so one crossing does not double-notify.
- **Exhausted tier (default GracefulStop).** `MessageKind::BudgetExhausted` emitted (feed + directed), then a `tokio::spawn` grace timer — production default 60s via `check_and_enforce_budget`, injectable via `check_and_enforce_budget_with_grace` so tests use 0. At expiry, still-`Active` agents (re-listed at kill time, so agents that went Idle during grace are spared) are killed through the shared `kill_agent` helper extracted from `admin_kill_agent` (clean cutover — the admin handler now delegates and keeps only its admin-specific task-blocking). Kill transitions to `Dead`, decrements the active-agent counter with a `was_active` guard (this also fixes a base leak: the old admin kill never decremented), preserves worktree/branch records (`preserve_worktree = true` skips the worktree cleanup the admin path performs), revokes the agent token, and creates the spec-titled reassignment task `"Agent hit budget limit on {task-id}, needs reassignment or budget increase"` with `parent_task_id` linkage plus an AgentEscalation notification to the spawner.
- **Overrides.** `HardKill` kills inline with no timer (test asserts `handle.is_none()` and Dead immediately); `QueuePause` pauses every workspace repo's merge queue through the real `merge_processor::pause_merge_queue` (kv `merge_queue_pause` keyed by repo id with `{"paused":true,...}`, `MergeQueuePaused` event, Admin/Owner notifications) and does not kill; `Warn` at 100% warns only. All three tested.
- **Per-period de-dup.** Persisted kv marker (`budget_enforcement` namespace, port-backed so it survives restarts) keyed to `period_start`: `warned`/`exhausted` each fire at most once per daily period, reset naturally when the midnight counter reset stamps a new `period_start`. Tests cover no-refire for both tiers.
- **Tests are real guards.** Every acceptance test drives the production funnel (`record_llm_call_usage`) or the real helper and asserts observable side effects (message counts via `list_by_workspace`/`list_after`, notification store contents, agent status, budget counter, worktree records, task store, kv state). Remove `check_and_enforce_budget` and the warn/below-threshold/kill tests fail on message counts and status; remove the kill path and the Dead/counter/worktree/reassignment assertions fail. No mirrored logic, no stubs, no assertionless tests. Below-threshold silence and cost-limit crossing are both covered.

## Blocked probe — verifier must run

`cargo test -p gyre-server --lib` is wrapper-rejected in this sandbox (4 GiB budget; exit 64, output saved). The independent verifier must execute:

- `cargo test -p gyre-server --lib api::budget` — task-191 acceptance tests (85% warn with message + notification + no kill + no re-fire; below-threshold silence; ≥100% graceful stop with Dead + counter 0 + worktree intact + reassignment task; HardKill inline; QueuePause kv state; cost-limit crossing; serde round-trips; API persistence) plus the upstream threshold-notification tests.
- `cargo test -p gyre-server --lib api::admin` — kill_agent cutover.
- `cargo test -p gyre-server --lib signal_chain` and `notifications` — fixture/`notify_rich` touchpoints.

Compile-level confidence for these is already established by `cargo check -p gyre-server --tests` (PASS).

## Non-blocking observations (recorded, not findings)

1. `emit_budget_warn`/`emit_budget_exhausted` loop over agents and emit one workspace-feed copy per agent, so a workspace with N active agents gets N identical feed copies (each agent still receives exactly one directed copy, and once-per-period dedup holds). Cosmetic over-emission on the feed only.
2. A server restart during the 60s grace window loses the detached timer while the persisted `exhausted` marker prevents re-fire, so agents spawned-Active before the restart survive un-killed until the next recording crosses a fresh period. Edge case; the spec does not address restart semantics.
3. Budget kills leave the killed agent's current task `InProgress` (admin kills block it); the reassignment task is the spec'd remediation and the spec does not mandate blocking. Defensible, documented in the shipped notes.
4. The task text named `NotificationType::BudgetWarning` (priority 7); the base (task-190) had already renamed it to `AgentBudgetWarning` with legacy `"BudgetWarning"` parse support, and this candidate routes through that single canonical notification path rather than forking a second one — the right call under the no-fork instruction; documented in the coverage matrix.
