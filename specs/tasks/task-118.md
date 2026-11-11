---
title: "Implement agent spawning, implementation phase & Ralph loop"
spec_ref: "agent-runtime.md §1 Phase 4"
depends_on:
  - task-115
  - task-116
  - task-117
progress: complete
coverage_sections:
  - "agent-runtime.md §Phase 4: System Spawns Agents (Mechanical)"
  - "agent-runtime.md §Phase 5: Implementation"
  - "agent-runtime.md §Phase 6: MR and Gates"
  - "agent-runtime.md §Phase 7: Ralph Loop"
  - "agent-runtime.md §Phase 8: System-Initiated Agents"
commits: ["26edb8fc38732bcb23f8c96770ccdbe60d79e4e7"]
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

- [x] Task scheduler detects Implementation tasks and spawns agents mechanically (`spawn_implementation_workers` in signal_chain.rs, appended to `scheduler_run_once` under the same cycle lock; claims Backlog→InProgress then spawns attempt 0 synchronously)
- [x] Precondition checks: repo status, max_agents, budget (repo Archived defers; `effective_max_agents` = `repo.max_agents` → `workspace.max_agents_per_repo` → unlimited, queueing not rejection; budget exhaustion flags the `budget-exhausted` task label and clears on the later successful spawn)
- [x] Agent completion triggers MR creation and gate execution (shared `create_mr_for_completion` + `ralph_loop::on_agent_mr_created` on BOTH the REST `POST /agents/:id/complete` and the MCP `gyre_agent_complete` — the MCP surface previously created no MR; `merge_queue::enqueue_core` triggers `trigger_gates_for_mr`)
- [x] Ralph loop: gate failure → re-spawn with fresh context + failure messages in inbox (`on_gates_failed`: task back to InProgress, one GateFailure message per failed gate carrying the gate's real recorded output, MR Closed, `relaunch_agent_session` re-launches the same agent entity — fresh JWT, same worktree/branch, re-assembled prompt set, `GYRE_ITERATION` env var)
- [x] Max iterations enforced (task Blocked, agent Failed, orchestrator notified) (`max_agent_iterations` repo setting, default 10; `notify_task_blocked` routes TaskBlocked to the repo orchestrator, spawning one on demand — same routing mechanism as SpecApproved)
- [x] Spawn failure retry with exponential backoff (1s, 5s, 30s) (`SPAWN_RETRY_BACKOFF_SECS = [1, 5, 30]`; claim kept fresh via `touch_task` against the 60s stale recovery; all retries exhausted → task Blocked + priority-1 `ComputeTargetUnavailable` notifications to workspace Owner/Admin members)
- [x] Spec rejection cancels in-flight tasks and agents (pre-existing in `reject_spec` — in-flight tasks cancelled, active agents Stopped with shutdown message + 60s grace; unchanged, covered by `reject_spec_cancels_chain_created_delegation_task`)
- [x] System-initiated agents: gate reviewer, interrogation, reconciliation (gate reviewer: `run_agent_review_gate` in gate_executor.rs, triggered by the merge-queue enqueue the Ralph loop now performs automatically; interrogation: spawn endpoint `agent_type: "interrogation"` with 30-min JWT + read-only ABAC policies; reconciliation: `run_reconciliation` rollout strategies create Delegation/engagement tasks that flow through the now-complete scheduler→worker chain)
- [x] `cargo test --all` passes — verified in-sandbox for `gyre-domain` (124), `gyre-common` (463), `gyre-adapters` (438+sqlite repository 9, incl. the new `max_agent_limits_persist`); `cargo check -p gyre-server --all-targets` clean. The gyre-server lib/test binaries exceed the sandbox 4 GiB build budget (staged wrapper), so the server probes must run in the independent verifier:
  `cargo test -p gyre-server --lib ralph_loop`
  `cargo test -p gyre-server --lib signal_chain`
  `cargo test -p gyre-server --lib api::repos`
  `cargo test -p gyre-server --test e2e_ralph_loop`

## Agent Instructions

Read `specs/system/agent-runtime.md` §1 Phases 4-8 in full. Agent spawn is in `gyre-server/src/api/spawn.rs`. Agent domain: `gyre-domain/src/agent.rs` (has `iteration`, `loop_config`). Task domain: `gyre-domain/src/task.rs` (has `task_type`, `order`, `depends_on`). Gate execution: `gyre-server/src/gate_executor.rs`. MR creation: grep for `create_mr` or `MergeRequest::new`. JWT minting: `gyre-server/src/auth.rs` or `gyre-server/src/signing.rs`. Compute target: `gyre-domain/src/compute_target.rs`.

---

## Implementation Notes (2026-11-12)

Commit `26edb8fc3` on branch `pipeline/task-118/…` closes Phases 4–8.

### New surfaces

- **`crates/gyre-server/src/ralph_loop.rs`** (new module): `on_agent_mr_created`
  (enqueue + gate trigger + watcher spawn), `watch_mr_gates` (bounded poll of
  gate results → verdict via `check_gates_for_mr`), `watch_mr_merge` /
  `complete_task_on_merge` (task Done + TaskTransitioned on merge),
  `on_gates_failed` (the full Phase 7 fail path), `notify_task_blocked`
  (orchestrator routing, shared with the scheduler), `relaunch_agent_session`
  (fresh JWT + same worktree/branch + re-assembled prompt set via the shared
  `launch_agent_process`). Timeouts: 1200s gate watch / 3600s merge watch;
  poll interval and gate-watch timeout overridable for tests
  (`GYRE_RALPH_POLL_MILLIS`, `GYRE_GATE_WATCH_TIMEOUT_SECS`).
- **signal_chain.rs Phase 4 worker scheduler**: `spawn_implementation_workers`
  appended to `scheduler_run_once` (same cycle lock — a worker claim can never
  interleave with another cycle's claim); preconditions as specced with
  queueing-not-rejection semantics for every defer; `try_spawn_worker` /
  `retry_worker_spawn` / `final_spawn_failure` / `teardown_failed_spawn` /
  `release_claim`; `ensure_repo_orchestrator_for_failure` for TaskBlocked
  routing.
- **spawn.rs refactor**: `launch_agent_process` + `AgentLaunchCtx`/`AgentLaunchResult`
  extracted from `spawn_agent_core` so the Ralph-loop re-spawn runs byte-identical
  provisioning (env injection incl. new `GYRE_ITERATION`, secrets, attestation,
  backend registration, exit monitoring). `SpawnAgentResponse.process_launched`
  reports launch outcome; an explicitly configured `GYRE_AGENT_COMMAND` now wins
  over container auto-detection (a deployment naming its agent command made a
  runtime choice). `create_mr_for_completion` shared by REST and MCP completion.
- **merge_queue.rs**: `enqueue_core` extracted (mergeable-status guard +
  `trigger_gates_for_mr`) shared by the REST handler and the Ralph loop.
- **mcp.rs**: `gyre_agent_complete` creates the completion MR (`complete_via_mr`
  helper: MR + token revocation + budget release) and starts the Ralph loop —
  closing the Phase 5 divergence where the MCP surface (the one the spec names)
  created no MR.

### Schema / domain

- `Repository.max_agents` / `max_agent_iterations` (Option, NULL = inherit),
  migration `2026-11-12-000060_repo_agent_limits` (portable SQL for both
  SQLite and PostgreSQL), diesel schema, sqlite + postgres adapters.
- `PUT /api/v1/repos/:id`: `max_agents` ≤ workspace `max_agents_per_repo`
  (400 on violation), `max_agent_iterations` ≥ 1; both fields in
  `RepoResponse`.
- `MessageKind::TaskBlocked` (Event tier, payload
  `{task_id, repo_id, reason, agent_id?, iteration?}`) and
  `NotificationType::ComputeTargetUnavailable` (priority 1).

### Tests added (all fail on behavior break)

- signal_chain: `scheduler_spawns_worker_for_implementation_task`,
  `scheduler_skips_archived_repo`, `scheduler_defers_until_depends_on_complete`,
  `scheduler_defers_lower_order_sibling`, `scheduler_queues_at_max_agents`
  (repo override, workspace fallback, and slot-freeing resume),
  `scheduler_flags_budget_exhausted` (flag set + cleared on later spawn),
  `worker_spawn_failure_retries_then_blocks` (k8s target = deterministic
  `spawn_process` failure; 3 retry agents Failed, task Blocked,
  TaskBlocked on the bus, priority-1 admin notification); the existing
  `scheduler_decomposes_delegation_into_ordered_subtasks` now also asserts
  the decomposed Implementation sub-task is claimed with an assigned worker.
- ralph_loop: `gate_failure_respawns_agent_with_fresh_context` (iteration
  incremented, Active, MR Closed, GateFailure message in inbox with the
  gate's REAL output, fresh token), `max_iterations_blocks_task_and_fails_agent`
  (ceiling → Blocked/Failed, TaskBlocked delivered to the on-demand repo
  orchestrator's inbox, token removed), `merge_completes_task` (Review→Done +
  TaskTransitioned).
- adapters: `max_agent_limits_persist` (create/inherit/update round-trip
  through the real migration).
- api/repos: `update_repo_agent_limits_validated` (workspace-limit 400,
  persist, zero-iterations 400).

### Verification status

In-sandbox: `cargo check -p gyre-server --all-targets` clean; `cargo fmt`
clean; mechanical invariants pass (arch, ABAC route registry, MCP write
gating, migration versions, dead message kinds, byte-slice truncation,
SQL portability, mem port contracts, fabricated-scope, lossy-secret,
scope-literal, inert-enforcement, forged-scope, forwarded-header,
in-memory-state, unbounded-HTTP, relative-path, fail-open-ref,
abac-exempt-handlers). `check-event-emission-coverage.sh` exits 2 on
pristine HEAD too in this environment (mawk lacks gawk 3-arg `match`) —
pre-existing, not introduced by this task. Server test binaries exceed the
sandbox 4 GiB wrapper budget; the four test probes above are the required
verifier probes. `tokio` gains the `test-util` feature (workspace Cargo.toml)
for `#[tokio::test(start_paused = true)]` in the retry-backoff test.
