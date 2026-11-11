# Review — task-118: Implement agent spawning, implementation phase & Ralph loop

Round: 4 (this round re-reviewed the full candidate `3f04c1bcc` against base
`65f71e65`, superseding the three prior repair rounds recorded in the task's
Shipped notes). Evidence: `/tmp/stage/review-evidence/` (README.md indexes
every probe, command, exit code, and log).

## Verdict

`progress: complete` — approved. The candidate implements agent-runtime.md
§1 Phases 4–8 as real production code; no stubs, audit-only paths, or
hollow coverage claims were found in the diff. Server test *execution* was
blocked by the executor's 4 GiB build wrapper (recorded, not bypassed); all
test code type-checks clean (`cargo check -p gyre-server --tests`), and
every smaller package's tests pass in-sandbox.

## What was verified

**Phase 4 (worker scheduler, signal_chain.rs `spawn_implementation_workers`).**
Runs inside `scheduler_run_once` under the cycle run lock. Mechanical
detection of `Backlog` + `task_type: Implementation` (Delegation/Coordination
and untyped push-hook tasks excluded); `depends_on` resolved (Done/Cancelled;
deleted dep treated resolved with a warn); `order` next (no incomplete
lower-order sibling in the repo+parent group); repo not Archived (defer,
resume on unarchive — queuing, not rejection); `effective_max_agents`
(repo.max_agents → workspace.max_agents_per_repo → unlimited) with
queue-not-429 semantics; workspace budget exhaustion flags the
`budget-exhausted` task label (persisted via the labels column) and clears it
on the later successful spawn. The claim is taken from a re-read freshest row
(defense against concurrent API mutation inside the cycle's await window),
then attempt 0 spawns synchronously through `spawn_agent_core` as the system
principal. `Repository.max_agents`/`max_agent_iterations` are new columns
(migration 000060 — next unused sequence, portable plain SQL, SQLite + PG +
diesel schema + mem updated symmetrically; `PUT /repos/:id` validates the
repo limit ≤ workspace per-repo limit, and the SQLite round-trip test
`max_agent_limits_persist` passes in-sandbox).

**Mid-flight claim release.** The round-2/3 repair is genuine:
`Task::release_claim` is a dedicated domain edge (the ONLY InProgress→
Backlog route; pinned by `test_transition_table_has_no_edge_into_backlog`),
used by `release_claim` when `spawn_agent_core`'s own budget check 429s after
the scheduler's pre-check passed. The regression test
`budget_exhausted_mid_flight_releases_claim_and_respawns` drives the exact
race (claim → exhaust → Blocked outcome → release with flag → restore →
re-spawn on a later cycle).

**Phase 5 (MCP completion creates the MR).** The audited divergence is
closed: `gyre_agent_complete` now goes through the shared
`create_mr_for_completion` core (MR from the agent's worktree branch → repo
default branch, token revoked, budget slot released), identical to the REST
`POST /agents/:id/complete`. A double-complete cannot mint a second watcher:
the agent transition table rejects Idle→Idle, so the second call errors
before MR creation.

**Phase 6 (gates run automatically).** Both completion surfaces call
`ralph_loop::on_agent_mr_created` → `merge_queue::enqueue_core`, which awaits
`trigger_gates_for_mr` — and that function synchronously persists a Pending
GateResult per gate *before* returning (gate_executor.rs:23-60), so the
Ralph watcher cannot vacuously pass on absent results. `enqueue_core` is
idempotent per open MR (re-enqueue returns the existing non-terminal entry;
a terminal entry allows a fresh retry entry; gates are not re-triggered for
an existing entry) — pinned by two merge_queue tests. The e2e test's
rewritten step 10 proves auto-enqueue non-vacuously (a "merged" MR can only
be reached through the queue).

**Phase 7 (Ralph loop, ralph_loop.rs).** Bounded gate watcher (20 min) →
merge watcher (1 h). Fail path: task back to InProgress (valid Review→
InProgress edge), one GateFailure message per failed gate in the agent's
inbox carrying the gate's real recorded output, failed MR Closed (Open→
Closed is a valid edge, so the next completion mints a fresh MR), re-spawn
of the SAME agent entity with a fresh JWT (kv agent_tokens), re-assembled
prompt set, same worktree/branch, `GYRE_ITERATION` injected (the shared
`launch_agent_process` env map covers container/k8s/local arms). Iteration
accounting: repo `max_agent_iterations` (migration 000060, PUT-validated
≥ 1) → default 10; at the ceiling the task is Blocked, the agent Failed,
the token removed, and `MessageKind::TaskBlocked` (new Event-tier kind with
schema, serde, and emitter — dead-kind check passes) is routed to the repo
orchestrator's inbox, spawning one on demand via the same mechanism as
SpecApproved. Spawn failure: 3 retries at exactly [1, 5, 30] seconds
(`SPAWN_RETRY_BACKOFF_SECS`), each failed attempt's agent torn down (Failed,
token removed, budget counter released — balanced against the core's
increment); exhausted → task Blocked + TaskBlocked + priority-1
`ComputeTargetUnavailable` notifications to workspace Owner/Admin members
(priority_rank 1 via the type mapping). Spec rejection mid-flight is
pre-existing and present (specs.rs:1263-1310: cancels in-flight tasks,
shutdown messages, 60s grace).

**Phase 8.** Interrogation agents (30-min JWT TTL, restrict-Deny policies)
pre-existing and genuine. Gate reviewer now triggered mechanically: the
Ralph loop enqueues every agent-authored completion MR, and
`trigger_gates_for_mr` → `run_agent_review_gate` spawns the reviewer
process. Reconciliation: meta-spec approval/binding change
(meta_specs.rs:303/1663) → `run_reconciliation` → deduplicated per-repo
reconciliation tasks → repo orchestrator → Implementation sub-tasks → the
newly-closed Phase 4 worker path with the full Ralph loop — which is the
spec's own description of the reconciliation agent's lifecycle ("Ralph loop
(same as worker agents)"). The coverage row states this mechanism honestly
rather than claiming a distinct agent type.

**Probes.** All 14 invariant scripts pass (the attribution script fails only
against the review setup's working-tree reset of the task file to
`commits: []`; the committed candidate records all four product commits).
`cargo check -p gyre-server` and `--tests` pass; `cargo clippy -p gyre-server`
exits 0; `cargo test -p gyre-domain` (466, incl. the three new
`release_claim` edge tests), `-p gyre-common` (124), and
`-p gyre-adapters -- sqlite::repository` (9, incl. `max_agent_limits_persist`)
pass. The one `cargo fmt --check` drift (gyre-ports/notification.rs) is
byte-identical at the assigned base — pre-existing, untouched by this
candidate.

## Verifier-required probes (wrapper-blocked here)

`cargo test -p gyre-server --lib ralph_loop`; `--lib signal_chain`;
`--lib merge_queue`; `--test e2e_ralph_loop`; then `cargo test --all`
(acceptance criterion). Full list with rationale in
`/tmp/stage/review-evidence/README.md`.

## Non-blocking observations (recorded for future hardening)

1. **Workspace iteration tier.** Phase 7 step 6 says max_iterations "falls
   back to workspace default, then server default of 10", but no platform
   spec or entity defines a workspace-level iteration default
   (repo-lifecycle.md §3 defines none; Workspace has no such field). The
   implementation falls back repo → 10 directly — the middle tier is a
   spec-internal inconsistency, not an implementable requirement. If the
   workspace tier is wanted, amend the spec and add the field; otherwise
   amend the Phase 7 sentence.
2. **Crash-recovery gap for worker claims.** An Implementation task claimed
   InProgress whose spawn is interrupted by a server crash (or a restart
   during the 36 s retry-backoff window) is stranded: the orchestrator
   loop's stale-claim recovery deliberately excludes Implementation tasks
   and the worker scan reads only Backlog. Manual remediation exists
   (InProgress → Blocked/Cancelled via the task API), and the spec does not
   require crash recovery for worker claims, so this is robustness
   hardening, not a contract gap. Note the `touch_task` comment in
   `retry_worker_spawn` overstates its purpose ("stale-claim recovery never
   re-claims") — stale recovery skips Implementation tasks entirely, so the
   touch is harmless dead protection today (and correct hardening if the
   recovery is ever extended).
3. **Test-env coupling.** `gate_failure_respawns_agent_with_fresh_context`
   sets the process-global `GYRE_AGENT_COMMAND=/bin/true` to pin the launch
   deterministically; parallel lib tests share the process env. No current
   test unsets it, so this is benign, but a future test that clears it
   would introduce ordering dependence.
4. **Supervised-trust workspaces.** The merge watcher gives up after 1 h on
   a trust-held MR, leaving the task in Review for human disposition — the
   documented platform trust model (task-095), not a Ralph-loop defect.
