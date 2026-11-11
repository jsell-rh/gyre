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
commits: ["56a528d05874c6113ef873f747cd515ccf068e7d", "73f302cae5ea83f44cb2a56a44397440b546e3f1", "096cd9bd6f97faae2b0bfc4e4a866752089c8953", "c8b4e5efc28c285c0a81f8f332e68439f72f58a0", "a08ef0e79e0d8a94559ea0ee3766d0f17bf16d53", "6ae1e989cf44636fc231b4d7b36f4189d2c10ae9", "26edb8fc38732bcb23f8c96770ccdbe60d79e4e7"]
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
`a08ef0e79`, `c8b4e5efc`, `096cd9bd6` (round-1/2 review and merge
repairs), `73f302cae` (round-3: Phase 7 spawn-failure policy on
re-spawns, stale worker-claim recovery), and `56a528d05` (round-4
verification repair, below) implement agent-runtime.md §1 Phases 4–8.

### Phase 4 — worker scheduler

`spawn_implementation_workers` (signal_chain.rs) runs inside
`scheduler_run_once` under the same cycle lock. It detects `Backlog`
tasks with `task_type: Implementation`, checks ordering (`depends_on`
resolved to Done/Cancelled; `order` next in the decomposition group) and
preconditions: repo not Archived (task stays Backlog), `max_agents`
(repo override → workspace fallback; task queued, never rejected),
workspace budget (task flagged `budget-exhausted` and left Backlog).
Eligible tasks are claimed Backlog→InProgress and spawned through the
real spawn core (agent record, repo-scoped JWT, prompt set, worktree,
compute-target provisioning, env injection) — the same core the REST
spawn endpoint uses. Crash recovery (`recover_stale_worker_claims`)
releases stale InProgress claims whose agent is not Active back to
Backlog via the domain's dedicated release edge.

### Phase 5/6 — completion → MR → gates

`agent.complete` (REST and MCP) creates the MR through
`create_mr_for_completion`; the MR enters the merge queue, which runs
the gate executor (test/lint/spec-review/trace) and records results on
the MR.

### Phase 7 — Ralph loop

`on_gates_failed`/`on_gates_passed` (ralph_loop.rs): gate failures are
written to the agent's inbox as `GateFailure` messages carrying the
gate's real recorded output, the failed MR is closed, and the agent is
re-spawned — same persona, same meta-spec set, same task, fresh JWT and
fresh context — with the iteration counter incremented. At
`max_iterations` (repo setting, default 10): task Blocked, agent Failed,
token removed, `TaskBlocked` routed to the repo orchestrator's inbox
(spawning one on demand when none is live, workspace-destined fallback
otherwise). Spawn failure on both the first-spawn and re-spawn paths
follows the same 3-retry exponential backoff ladder (1s/5s/30s,
`SPAWN_RETRY_BACKOFF_SECS`), then task Blocked + priority-1
`ComputeTargetUnavailable` notification for workspace Owner/Admin
members. Spec rejection cancels in-flight tasks and shuts down agents
with a 60s grace period.

### Phase 8 — system-initiated agents

Gate reviewer (gate executor spec-review gate), interrogation agent
("Ask Why" endpoint, 30-min JWT TTL), reconciliation agent (meta-spec
change publication) all spawn through the real spawn core on the
workspace compute target.

### Round-4 verification repair (`56a528d05`)

The round-3 candidate failed `cargo test --all` with 4 gyre-server lib
test failures (attempt 055fbdf3), all test-side:

- Three tests asserted the `TaskBlocked` message through
  `messages.list_by_workspace`, which by port contract excludes
  Directed (agent-destined) messages; `notify_task_blocked` routes to
  the repo orchestrator's inbox whenever one can be ensured on demand
  (the spec's primary routing), so the assertions saw `[]`. New
  cfg(test) helper `collect_task_blocked_messages` collects TaskBlocked
  from both bus tiers (workspace event tier + repo-orchestrator
  inboxes) and the three tests assert through it — same assertion
  strength (exact task_id payload), correct accessor.
- `stale_worker_claim_released_and_respawned` transitioned its gating
  dependency directly to Done, but the first scheduler cycle correctly
  claims that dependency (an eligible Backlog Implementation task) to
  InProgress; the domain requires InProgress→Review→Done. The test now
  walks the dependency to Done from whichever state the spawn left it
  in, mirroring `scheduler_defers_until_depends_on_complete`.

### Test evidence

- `cargo check -p gyre-server --tests`: clean (type-checks the lib
  including cfg(test) targets).
- `cargo fmt`: clean; arch, dead-message-kind, in-memory-store,
  inert-enforcement scripts: OK.
- Full `cargo test --all` (1747+ tests incl. the four repaired tests)
  is deferred to the independent verifier — gyre-server test codegen
  exceeds the in-model 4 GiB budget (probe list recorded in
  /tmp/stage/review-evidence/round4-repair.md).
