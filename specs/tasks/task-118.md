---
title: "Implement agent spawning, implementation phase & Ralph loop"
spec_ref: "agent-runtime.md §1 Phase 4"
depends_on:
  - task-115
  - task-116
  - task-117
progress: ready-for-review
coverage_sections:
  - "agent-runtime.md §Phase 4: System Spawns Agents (Mechanical)"
  - "agent-runtime.md §Phase 5: Implementation"
  - "agent-runtime.md §Phase 6: MR and Gates"
  - "agent-runtime.md §Phase 7: Ralph Loop"
  - "agent-runtime.md §Phase 8: System-Initiated Agents"
commits: ["73f302cae5ea83f44cb2a56a44397440b546e3f1", "096cd9bd6f97faae2b0bfc4e4a866752089c8953", "c8b4e5efc28c285c0a81f8f332e68439f72f58a0", "a08ef0e79e0d8a94559ea0ee3766d0f17bf16d53", "6ae1e989cf44636fc231b4d7b36f4189d2c10ae9", "26edb8fc38732bcb23f8c96770ccdbe60d79e4e7"]
---

## Spec Excerpt

From `agent-runtime.md` §1 Phases 4-8:

**Phase 4 — System Spawns Agents:**
Task scheduler detects `Backlog` tasks with `task_type: Implementation`. Checks ordering (`depends_on` complete, next `order` value). Preconditions: repo not Archived, under `max_agents` limit, workspace budget not exhausted. Then: sets task `InProgress`, creates agent record (`Active`), mints repo-scoped JWT, assembles prompt set (§2), creates worktree, provisions on compute target (§3), injects env vars.

**Phase 5 — Implementation:**
Agent works autonomously: reads task via MCP, edits files, runs tests, commits, pushes, calls `agent.complete`.

**Phase 6 — MR and Gates:**
On agent completion: server creates MR, gates run automatically (test, lint, spec review, trace capture). All results recorded on MR.

**Phase 7 — Ralph Loop:**
- Gates pass → MR merges, task Completed, agent Idle, JWT revoked
- Gates fail → gate failures written to agent inbox, server re-spawns agent (same persona, same meta-specs, fresh context), agent reads failures and re-implements. Loop until convergence or `max_iterations` (default 10, configurable per repo).
- Max iterations → task Blocked, agent Failed, `TaskBlocked` message to repo orchestrator
- Spawn failure → 3 retries with exponential backoff, then task Blocked + priority-1 notification
- Spec rejection mid-flight → cancel tasks, shutdown agents (60s grace)

**Phase 8 — System-Initiated Agents:**
- Gate reviewer: spawned when MR enters gate queue
- Interrogation: spawned on "Ask Why" click (30-min JWT TTL)
- Reconciliation: spawned on meta-spec change

## Implementation Plan

1. **Task scheduler service:**
   - Background service that polls for `Backlog` + `Implementation` tasks
   - Checks `depends_on` completion and `order` sequencing
   - Precondition checks: repo status, max_agents count, workspace budget
   - On eligible task: transition to `InProgress`, create agent, mint JWT, assemble prompts (via task-116), create worktree, spawn on compute target (via task-117)

2. **Agent lifecycle automation:**
   - Wire `agent.complete` handler to trigger MR creation
   - After MR creation, trigger gate execution
   - After gate execution, check results:
     - All pass → merge MR, mark task Done, mark agent Idle, revoke JWT
     - Any fail → write gate failure to agent inbox, re-spawn agent (fresh context)
   - Track iteration count on agent entity

3. **Ralph loop re-spawn:**
   - On gate failure: increment `iteration` on agent, check against `max_iterations`
   - If under limit: re-spawn with same persona + meta-specs + task, but fresh context
   - Gate failure messages delivered to agent inbox so new session reads them
   - If at limit: mark task `Blocked`, agent `Failed`, notify repo orchestrator

4. **Spawn failure handling:**
   - Retry up to 3 times with exponential backoff (1s, 5s, 30s)
   - All retries fail: task Blocked, priority-1 notification for workspace admins

5. **Spec rejection mid-flight:**
   - On `POST /specs/:path/reject`: cancel in-flight tasks with that spec_ref
   - Send shutdown message to active agents, 60s grace period
   - Cancel delegation task if any

6. **System-initiated agents:**
   - Gate reviewer: spawned by gate executor when spec-review gate runs
   - Interrogation agent: spawned by "Ask Why" endpoint
   - Reconciliation agent: spawned on meta-spec change publication

## Acceptance Criteria

- [ ] Task scheduler detects Implementation tasks and spawns agents mechanically
- [ ] Precondition checks: repo status, max_agents, budget
- [ ] Agent completion triggers MR creation and gate execution
- [ ] Ralph loop: gate failure → re-spawn with fresh context + failure messages in inbox
- [ ] Max iterations enforced (task Blocked, agent Failed, orchestrator notified)
- [ ] Spawn failure retry with exponential backoff (1s, 5s, 30s)
- [ ] Spec rejection cancels in-flight tasks and agents
- [ ] System-initiated agents: gate reviewer, interrogation, reconciliation
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §1 Phases 4-8 in full. Agent spawn is in `gyre-server/src/api/spawn.rs`. Agent domain: `gyre-domain/src/agent.rs` (has `iteration`, `loop_config`). Task domain: `gyre-domain/src/task.rs` (has `task_type`, `order`, `depends_on`). Gate execution: `gyre-server/src/gate_executor.rs`. MR creation: grep for `create_mr` or `MergeRequest::new`. JWT minting: `gyre-server/src/auth.rs` or `gyre-server/src/signing.rs`. Compute target: `gyre-domain/src/compute_target.rs`.

## Shipped

Commits `26edb8fc3` (Phases 4–8 implementation), `6ae1e989c`,
`a08ef0e79`, `c8b4e5efc` (round-1 review/verification repairs),
`096cd9bd6` (merge repair with main: `QualityGate.validation_type` in
the ralph_loop fixture), and `73f302cae` (round-2 review repairs F1–F3,
below) implement agent-runtime.md §1 Phases 4–8.

### Phase 4 — worker scheduler

`spawn_implementation_workers` (signal_chain.rs) runs inside
`scheduler_run_once` under the same cycle lock. It detects `Backlog`
tasks with `task_type: Implementation`, checks ordering (`depends_on`
resolved; `order` next in the repo+parent group) and preconditions:
repo not `Archived` (defer, resume on unarchive); `effective_max_agents`
= `repo.max_agents` → `workspace.max_agents_per_repo` → unlimited, with
queueing-not-rejection semantics; workspace budget exhaustion flags the
`budget-exhausted` task label (cleared on the later successful spawn).
It re-reads the freshest task row before claiming, claims the task
`InProgress`, and spawns attempt 0 synchronously through
`spawn_agent_core` as the system principal. New
`Repository.max_agents`/`max_agent_iterations` columns (migration
000060, SQLite + PG; `PUT /repos/:id` validates repo limit ≤ workspace
per-repo limit).

Mid-flight budget race (claim taken, budget exhausted inside the spawn
core): the claim is released to Backlog via the domain edge
`Task::release_claim` — the ONLY InProgress→Backlog transition — with
the `budget-exhausted` label, and a later cycle re-spawns the task
(`budget_exhausted_mid_flight_releases_claim_and_respawns`).

Crash recovery for claimed worker tasks (round-3 F2 repair):
`recover_stale_worker_claims` releases a stale InProgress Implementation
claim whose assigned agent is not Active — e.g. a server crash during
the spawn-retry backoff, when the last attempt's agent is already
Failed — back to Backlog through the same `release_claim` edge, so a
later cycle re-spawns it. A claim held by an Active agent is never
released (task rows are not touched while a worker runs); a
crashed-but-Active agent is first reaped by the stale-agent detector.
Tests: `stale_worker_claim_released_and_respawned`,
`stale_worker_claim_with_active_agent_is_kept`.

### Phase 5→6 — completion, MR and gates

Both completion surfaces — REST `POST /agents/:id/complete` and MCP
`gyre_agent_complete` — share `create_mr_for_completion` (idempotent per
open agent+branch MR) and call `ralph_loop::on_agent_mr_created`, which
enqueues via `merge_queue::enqueue_core`; enqueue triggers
`trigger_gates_for_mr`, so gates run automatically and results are
recorded on the MR. `tests/e2e_ralph_loop.rs` proves auto-enqueue
without a manual step.

### Phase 7 — Ralph loop

Pass path: bounded gate watcher → merge-processor merge observed → task
Review→Done with `TaskTransitioned`; agent Idle + JWT revoked at
completion. Closed-MR and trust-hold cases stop the loop with the task
left in Review (human decision).

Fail path: task back to InProgress, one `GateFailure` inbox message per
failed gate carrying the gate's real recorded output, failed MR Closed,
iteration incremented against `Repository.max_agent_iterations`,
re-spawn through the same `launch_agent_process` core with fresh JWT,
`GYRE_ITERATION` injected, prompt set re-assembled, budget re-claimed.
Max iterations → task Blocked, agent Failed, token removed, `TaskBlocked`
routed to a spawned-on-demand repo orchestrator.

Spawn-failure policy on BOTH spawn paths (round-3 F1 repair): the re-spawn
launch is a `ComputeTarget::spawn_process()` call and the Phase 7 policy
is not path-scoped — `respawn_with_retry` applies the same 1s/5s/30s
exponential backoff ladder (`SPAWN_RETRY_BACKOFF_SECS`, shared with
`retry_worker_spawn`) before any terminal state, keeps the claim fresh
per attempt (`touch_task`), and on exhaustion marks the task Blocked,
the agent Failed, removes the freshly-minted re-spawn JWT (agent JWT
auth validates token presence + signature, not agent status), notifies
the repo orchestrator (`TaskBlocked`) AND creates the priority-1
`ComputeTargetUnavailable` notification for workspace admins
(`notify_compute_target_unavailable`, extracted from
`final_spawn_failure` so both ladders share it). Test:
`respawn_failure_retries_then_blocks_task` (paused clock; asserts the
36s backoff ladder, terminal state, JWT removal, both notifications).

### Phase 8 — system-initiated agents

Gate reviewer (single-shot reviewer process, `GYRE_REVIEW_TOKEN`)
triggered mechanically by the Ralph loop's auto-enqueue; interrogation
agents (30-min JWT, read-only, no worktree) on "Ask Why"; reconciliation
tasks flow through the closed Phase 4 spawn chain. Spec rejection
mid-flight cancels in-flight tasks by spec_path, stops active agents
with a 60s-grace shutdown message, priority-2 notifications.

### Test evidence and deferred verification

In-sandbox (implement + review stages): `cargo check -p gyre-server
--lib --tests` clean; `cargo fmt --check` clean; gyre-domain (466),
gyre-common (124), gyre-adapters sqlite::repository (9) suites green
(review stage); all repo invariant scripts pass. Server test EXECUTION
is blocked by the build wrapper in both the review and implement
sandboxes (4 GiB budget) — recorded as an infrastructure restriction,
not a code defect. The independent verifier must run: `cargo test
--all`, plus focused `cargo test -p gyre-server ralph_loop`,
`signal_chain`, `merge_queue`, `update_repo_agent_limits_validated`,
and `--test e2e_ralph_loop`. Round 3 also fixed a latent never-executed
test bug (`scheduler_defers_until_depends_on_complete` walked its
spawned dependency from Backlog unconditionally — invalid same-state
transition) and removed the `GYRE_AGENT_COMMAND` env leak from
`gate_failure_respawns_agent_with_fresh_context` (F3, Drop guard).
