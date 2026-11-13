# Review — task-191: budget enforcement actions (warn/graceful-stop/kill/reassignment)

**Candidate:** 9a2514b82b94e2e5b221bb5a6272d6ab01f244ca (branch
`pipeline/task-191/9beceef12fee4762b5d0df1d01bed810-1`, base 23d7361f)
**Verdict:** APPROVED (independent review, this round)
**Evidence:** /tmp/stage/review-evidence/probes.md (+ per-probe logs)

## What was reviewed

The rebase-repair round unifying this task's platform-model.md §5
Enforcement Behavior requirements with task-119's agent-runtime.md §4
engine that landed on main while PR 808 was published. Diff vs base:
`gyre-domain/src/budget.rs` (enum + config field), migration
2026-10-10-000058 + SQLite/PG row mapping + repo `budget_json` round-trip,
`api/budget.rs` (action dispatch, per-period exhaustion marker,
warn-tier agent-inbox delivery, reassignment task, QueuePause),
`api/admin.rs` (`kill_agent` extraction, double-decrement fix),
`signal_chain.rs` test fixtures, docs, coverage matrix row 28.

## Findings: none blocking

- **Real implementations only.** `BudgetEnforcementAction` (4 variants,
  serde snake_case) lives in `gyre-domain`, persists through SQLite + PG
  adapters, the repo-level `budget_json` document, and the budget API
  DTOs — round-trip tests at every layer (adapters suite ran green here).
  Threshold enforcement computes max utilization across token/cost limits
  against the effective (workspace ∩ tenant) config, with
  divide-by-zero/None guards. All four actions have distinct, observable
  behavior: GracefulStop (default) emits `BudgetExhausted` to every active
  agent inbox, arms the single-per-workspace 60s grace timer (injectable
  `grace_secs`), re-evaluates before killing, fail-closed on usage read
  error; HardKill kills inline; QueuePause pauses every workspace repo's
  merge queue via the shared `merge_processor::pause_merge_queue` without
  killing; Warn keeps the warn tier. Every budget stop preserves the
  worktree, leaves the task InProgress, revokes the token, charges compute
  minutes, releases the active-agent slot, and creates the spec-exact
  reassignment task ("Agent hit budget limit on {task-id}, needs
  reassignment or budget increase", parent-linked, High).
- **No forked engine.** The candidate's pre-rebase parallel engine is fully
  removed (grep: zero matches); `enforce_workspace_budget` is the single
  path, invoked after every usage report and budget PUT.
- **Spec reconciliation is genuine, not reinterpretation.** Budget kills
  mark agents `Stopped` (not Dead) and notify via `AgentBudgetWarning`/
  `AgentBudgetExhausted` — verified against the canonical amended
  agent-runtime.md §4 text at the base commit, which mandates exactly this;
  the task sketch's "Dead"/"BudgetWarning (priority 7)" predate the
  taxonomy unification. Beyond-limit immediate kill (budget reduced below
  usage) preserved regardless of action override, per the same spec.
- **De-dup is real persisted state.** Warn tier via
  `has_active_notification` (type + entity + unresolved + since
  period_start); exhaustion via a kv-backed marker keyed to
  `usage.period_start` — survives restarts, resets with the rolling window.
- **Tests are kill-tests, not coverage theater.** They drive the production
  entry points and assert on real stores (messages, notifications, agent
  status, worktrees, tasks, kv). The HardKill test passes grace=60 so a
  GracefulStop misroute leaves the agent alive and fails; the warn test
  re-records in-period and asserts no re-fire; below-threshold asserts
  silence. Traced assertion-by-assertion: each would fail with the
  corresponding production behavior removed.

## Probes run (this sandbox)

`cargo check -p gyre-server --tests` (0 errors); `cargo test -p gyre-domain`
(507 passed); `cargo test -p gyre-adapters` (490 passed, 6 PG-ignored,
incl. enforcement_action round-trip); check-arch, migration-versions,
dead-message-kinds, inert-enforcement, migration-sql-portability,
mem-port-contracts, task-commit-attribution — all OK.

## Verifier must run (server binaries exceed the 4 GiB review sandbox)

`cargo test -p gyre-server api::budget`, `api::admin`, `signal_chain`,
`mcp`, and `cargo build --all`.

## Non-blocking observations

1. Warn-tier bus messages ride the human-notification level-crossing gate
   (`created > 0`): an Admin-less workspace with non-agent-attributed usage
   (llm_query) crossing 80% emits no `BudgetWarning` message. Agent-attributed
   usage always has the spawner recipient; the exhaustion tier is not gated.
2. Exhaustion fires at most once per daily period: a mid-period budget raise
   followed by re-exhaustion in the same period does not re-fire until the
   period rolls (matches the task's dedup instruction; documented in code).
