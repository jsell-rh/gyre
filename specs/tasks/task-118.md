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
commits: ["26edb8fc38732bcb23f8c96770ccdbe60d79e4e7", "6ae1e989cf44636fc231b4d7b36f4189d2c10ae9"]
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

Commit `26edb8fc3` implements agent-runtime.md §1 Phases 4–8; commit
`6ae1e989c` repairs the independent-review findings on that
implementation. Behavior as it stands on this branch:

### Phase 4 — worker scheduler

`spawn_implementation_workers` (signal_chain.rs) runs inside
`scheduler_run_once` under the same cycle lock. It detects `Backlog`
tasks with `task_type: Implementation` (Delegation/Coordination
excluded), checks ordering (`depends_on` resolved; `order` is next in
the repo+parent group), and preconditions: repo not `Archived` (defer,
resume on unarchive); `effective_max_agents` = `repo.max_agents` →
`workspace.max_agents_per_repo` → unlimited, with
queueing-not-rejection semantics; workspace budget exhaustion flags
the `budget-exhausted` task label (cleared on the later successful
spawn). It re-reads the freshest task row before claiming (and before
flagging) so a concurrent API mutation inside the cycle's await
window is not overwritten by the whole-row persist. It claims the
task `InProgress` and spawns attempt 0 synchronously through
`spawn_agent_core` as the system principal. New
`Repository.max_agents`/`max_agent_iterations` columns (migration
000060, SQLite + PG; `PUT /repos/:id` validates repo limit ≤
workspace per-repo limit).

### Phase 6 — MR and gates on completion

Both completion surfaces — REST `POST /agents/:id/complete` and MCP
`gyre_agent_complete` (which previously created no MR) — share
`create_mr_for_completion` and call `ralph_loop::on_agent_mr_created`,
which enqueues via `merge_queue::enqueue_core`; enqueue triggers
`trigger_gates_for_mr` so gates run automatically and results are
recorded on the MR. `enqueue_core` is idempotent per open MR (review
repair): a manual enqueue of an already-queued MR returns the
existing non-terminal entry (200 OK) instead of a duplicate that
could never be merged twice; a terminal entry still allows a fresh
enqueue (retry path); gates are not re-triggered for an existing
entry.

### Phase 7 — Ralph loop

New `crates/gyre-server/src/ralph_loop.rs`. Pass path: a bounded
watcher waits for the merge processor's merge, then task → Done +
TaskTransitioned, agent Idle, JWT revoked. Fail path: task back to
`InProgress`, one `GateFailure` message per failed gate in the
agent's inbox carrying the gate's real recorded output, failed MR
Closed, and `relaunch_agent_session` re-launches the same agent
entity with a fresh JWT and fresh context on the same
worktree/branch (iteration incremented, `GYRE_ITERATION` injected,
prompt set re-assembled at pinned versions). At
`max_agent_iterations` (repo setting, default 10): task `Blocked`,
agent `Failed`, token removed, `TaskBlocked` routed to the repo
orchestrator (spawned on demand). Spawn failure: 3 retries at
1s/5s/30s (`SPAWN_RETRY_BACKOFF_SECS`, claim kept fresh vs the 60s
stale recovery; covered by `worker_spawn_failure_retries_then_blocks`
under `#[tokio::test(start_paused = true)]` — tokio `test-util` is a
gyre-server dev-dependency, not a production feature); exhausted →
task `Blocked` + priority-1 `ComputeTargetUnavailable` notifications
to workspace Owner/Admin members. Spec rejection mid-flight
cancellation was already genuine (specs.rs, unchanged).

### Phase 8 — system-initiated agents

Gate reviewer (`run_agent_review_gate`, gate_executor.rs) is
triggered mechanically now that the Ralph loop enqueues every agent
MR; interrogation agents (spawn endpoint `agent_type:
"interrogation"`, 30-min JWT, read-only ABAC policies) and
reconciliation rollouts (`run_reconciliation` creating tasks that
flow through the now-complete scheduler→worker chain) were
pre-existing and verified.

### Test evidence

Verified in-sandbox at `6ae1e989c`:
`cargo check -p gyre-server --all-targets` clean (no warnings); the
workspace feature change keeps every crate type-checking. The
gyre-server lib/test binaries exceed the sandbox 4 GiB build budget,
so the server probes must run in the independent verifier:

- `cargo test -p gyre-server --lib ralph_loop`
- `cargo test -p gyre-server --lib signal_chain`
- `cargo test -p gyre-server --lib api::merge_queue` (new:
  `enqueue_is_idempotent_per_open_mr`,
  `enqueue_after_terminal_entry_creates_fresh_entry`)
- `cargo test -p gyre-server --test e2e_ralph_loop` (step 10 now
  proves the Phase 6 auto-enqueue with a no-duplicate assertion and
  no manual enqueue of the already-queued MR)
- `cargo test -p gyre-server --test git_integration` (its merge-queue
  test completes an agent and then manually enqueues the same MR —
  now exercises the idempotent path)
- `cargo test --all` (task acceptance criterion)
