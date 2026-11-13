# Review — task-191 (Budget enforcement actions: warn/graceful-stop with grace, kill, reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior.
Candidate: `17088a61` (base `ccfd4bc5`); product commits `b3dea290` → `35f2bba7` (repair) → docs `17088a61`.
Verdict: **approved** — the platform-model §5 Enforcement Behavior section is genuinely implemented, unified with task-119's agent-runtime §4 engine (no fork), and the prior verification failures are precisely repaired.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain` → pass.
- `cargo test -p gyre-adapters --lib budget` → 8/8 pass, incl. the new SQLite `enforcement_action` round-trip (set/get + overwrite-to-None) and the extended repo-level `budget_json` round-trip.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → pass (server test *execution* is deferred to the verifier per the 4 GiB build wrapper; the wrapper explicitly rejected a test-profile check).
- All mechanical lints pass: check-arch, check-migration-versions (new migration `2026-10-10-000058` derives a unique diesel version; `budget_configs` is created by an earlier-sorting migration), check-dead-message-kinds (BudgetWarning/BudgetExhausted now have live emitters), check-mem-port-contracts, check-inert-enforcement, fabricated-scope, scope-literal, in-memory-state-stores, lossy-secret, unbounded-http.

## Verified against the acceptance criteria

- **Enum + persistence.** `BudgetEnforcementAction` (Warn/GracefulStop/HardKill/QueuePause, serde snake_case) in `gyre-domain/src/budget.rs`, re-exported; round-trip test covers all four variants plus both `BudgetConfig.enforcement_action` states. Threaded through migration 000058 (portable plain `ALTER TABLE ... ADD COLUMN` for both backends), SQLite + PG row mapping (mirrored encode/decode), the repo-level `budget_json` document, and the `SetBudgetRequest`/`BudgetResponse` DTOs — PUT/GET round-trip tested through the real router (`enforcement_action_persists_through_budget_api`). Unknown variant strings are rejected by serde at the API boundary.
- **Single enforcement engine, wired at the recording sites.** `enforce_workspace_budget` runs after every `record_llm_call_usage` (agent usage endpoint, MCP, specs_assist, graph — the task-190 real counters) and after every workspace budget PUT. No threshold logic duplicated at call sites. `effective_workspace_config` threads the action per level (workspace override, else tenant, else spec default).
- **80% tier.** Utilization is max(tokens, cost) with None/zero guards (`utilization_pct`). Crossing emits `MessageKind::BudgetWarning` on the workspace feed AND a directed copy to every active agent's MCP inbox, plus `NotificationType::AgentBudgetWarning` to the affected agents' spawners + workspace Admins, de-duplicated per period (`has_active_notification` gates re-emission). Below-threshold silence is tested (over-firing guard). Agents are not killed.
- **≥100% tier.** Fires at most once per daily period (persisted kv marker keyed to `usage.period_start`, so it survives restarts and resets with the rolling window). Default `GracefulStop`: `BudgetExhausted` to every active agent inbox, 60s grace (`BUDGET_GRACE_PERIOD_SECS = 60`, injectable — tests use 0), single live timer per workspace (`budget_grace_in_progress` guard), re-evaluates utilization before killing and fails closed on usage-read error. Still-active agents are stopped via `budget_stop_agent`: process killed through the spawn-recorded compute backend, token revoked, `AgentKilled` audit emitted, compute minutes charged, active-agent slot released, worktree/branch preserved, task left InProgress, and the spec-exact reassignment task "Agent hit budget limit on {task-id}, needs reassignment or budget increase" created (parent-linked, High). Overrides: `HardKill` kills inline (tested with grace=60 to prove no timer dependence); `QueuePause` pauses every workspace repo's merge queue via the shared `merge_processor::pause_merge_queue` (kv state asserted) without killing; `Warn` keeps the warn tier. Beyond-limit on budget reduction keeps the immediate hard kill per agent-runtime.md §4.
- **Terminal state `Stopped`, not `Dead`.** The task plan sketched "transition to Dead", but agent-runtime.md §4 (line 418) explicitly mandates "Agents killed after the grace period are marked `Stopped`" with work preserved — the canonical, more specific spec. The candidate follows the spec and documents the reconciliation; the acceptance essence (process killed, counters decremented, work preserved, reassignment task) is fully met.
- **`kill_agent` helper extraction.** The admin kill sequence (Dead transition, process kill, worktree cleanup, token/secret revocation, audit events, slot release) is extracted into `admin.rs::kill_agent` and the admin handler migrated to it (clean cutover, no duplication). Budget stops deliberately use the pre-existing `budget_stop_agent` with materially different terminal semantics (Stopped/InProgress/worktree-preserved) mandated by agent-runtime.md §4 — documented in code; this is the right reconciliation, not a fork.

## Prior-round failure repair (durable findings, `cargo test --all` exit 101 at parent 9a2514b)

Both failures were root-caused in `notify_budget_level` deriving recipients only from the (absent) trigger agent, so unattributed crossings produced zero recipients → `created == 0` gated off emissions. Commit `35f2bba7` adds the spawning users of the workspace's *active* agents to the recipient list and emits the workspace-feed warning on every warn-tier crossing (agent_id in payload only when attributed). Both fixed paths re-verified by line-level reading against the mem-backed message/notification repositories. Crucially, at 9a2514b the graceful-stop test failed only at its final spawner-notification assertion — every kill-path assertion (inbox delivery, Stopped, active_agents → 0, worktree preserved, reassignment task) already passed in a real full-suite run, and all other task-191 tests passed (1914/1916). The fix touches production code only; test code is unchanged.

## Verifier must run (not executable in the review sandbox)

- `cargo test -p gyre-server --lib api::budget`
- `cargo test --all`
- `cargo build --all`

## Minor observations (non-blocking)

- Warn-tier agent-inbox/feed emission is gated on `created > 0` (human notification level-crossing). Every spawned agent has `spawned_by` set (spawn.rs), so recipients exist whenever the warn tier has an agent audience; a workspace with zero recipients and zero active agents suppresses only the feed event, for which the spec defines no recipient.
- `create_budget_reassignment_task` falls back to `Id::new("")` for `repo_id`, identical to `Task::new`'s domain default — not a fabricated scope.

Test-inflation check: none found. The enforcement tests drive the production entry points (`record_llm_call_usage` / `enforce_workspace_budget`) against real mem-backed stores, assert on observable state (message counts by destination, agent status, counters, worktree records, task titles, kv pause state), include negative guards (below-threshold silence, no re-fire within a period, HardKill distinguished from timer-dependent GracefulStop at grace=60), and demonstrably fail when the emission paths break (the prior round's failures were exactly such breaks).
