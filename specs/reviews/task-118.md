# Review — task-118: Implement agent spawning, implementation phase & Ralph loop

Round: 1. Candidate `b14eb0590` (production commit `26edb8fc3`), base
`75afe7fca`. Reviewer scope: full diff base..candidate (23 files, +2698/−187).
Full evidence, probe logs, and exit codes:
`/tmp/stage/review-evidence/review-summary.md`.

## Verdict: NOT approved (1 blocking finding, 2 minor)

### F1 (blocking): merge-queue double enqueue breaks the pre-existing e2e test

`complete_agent` now ends with `ralph_loop::on_agent_mr_created`, which
enqueues every agent-authored completion MR via `merge_queue::enqueue_core`.
`enqueue_core` has no already-queued guard, and the
`MergeQueueRepository::enqueue` port has no dedup in any adapter (mem pushes,
sqlite inserts; no unique constraint on `merge_request_id`). The pre-existing
`tests/e2e_ralph_loop.rs::full_ralph_loop_via_gyre` — unchanged by this
candidate — manually POSTs `/merge-queue/enqueue` for the same MR milliseconds
later and asserts `GET /merge-queue` returns exactly **1** entry. With the
automatic enqueue there are two `Queued` entries for the MR:

- No merge-processor tick in the window → `len == 2` → the len==1 assertion
  fails.
- Tick between completion and the manual enqueue → entry A merges, the MR goes
  `Merged`, the manual enqueue returns 409 Conflict → the
  `status == "queued"` assertion fails.
- Passing requires a 5s-period tick to land inside the few-ms window between
  the manual-enqueue response and the queue GET.

Verified statically at the candidate: `MergeRequest::new` starts `Open`
(merge_request.rs:118), so both enqueues pass the mergeable-status guard;
`list_queue` (mem.rs:1162) returns all non-terminal entries; the merge
processor ticks every 5s (merge_processor.rs:19). The wrapper forbids building
gyre-server test binaries in the review stage (4 GiB budget), so the decisive
execution is deferred to the verifier:
`cargo test -p gyre-server --test e2e_ralph_loop` — expected to FAIL.
This violates the acceptance criterion "`cargo test --all` passes".

Production symptom of the same root cause: any manual enqueue (UI retry or
REST) of an agent MR the Ralph loop already auto-enqueued creates a duplicate
queue entry that later fails with "cannot re-merge" noise.

Fix direction: make `enqueue_core` idempotent per open MR (return the existing
non-terminal entry instead of inserting a second one) and/or update the e2e
test to not manually enqueue what completion already enqueues.

### F2 (minor): tokio `test-util` in production dependencies

`Cargo.toml` adds the `test-util` feature to the workspace production tokio
dependency, needed only by `#[tokio::test(start_paused = true)]` in one
signal_chain test. Every production build now compiles tokio test-only code.
Dev-only feature re-enable is the conventional shape. Not functional.

### F3 (minor): stale-snapshot claim in the Phase 4 worker path

`spawn_implementation_workers` claims Backlog tasks by whole-row persisting
the snapshot from `list_by_status`, without the re-read-after-lock defense the
orchestrator loop in the same `scheduler_run_once` applies
(signal_chain.rs:650-657). A concurrent API edit to the task inside the
per-task await window is silently overwritten. Narrow race.

## Verified-correct (independently probed in this sandbox)

- All mechanical invariant scripts pass at the candidate (arch, ABAC route
  registry, dead message kinds, migration versions/portability, mem port
  contracts, inert enforcement, byte-slice, MCP write tools, exempt handlers,
  scope/forgery/forwarded/lossy/in-memory-state/unbounded-HTTP checks); the
  five scripts that exit non-zero fail identically at the base commit
  (pre-existing).
- `cargo test -p gyre-common` (463), `-p gyre-domain` (124+8),
  `-p gyre-adapters` (439, incl. new `max_agent_limits_persist`) all pass;
  `cargo check -p gyre-server` clean.
- Task-commit attribution passes on the committed candidate tree
  (`26edb8fc3` is in task-118's `commits:` frontmatter).
- Phase 4 scheduler logic is genuine against the spec: Backlog +
  `task_type: Implementation` discrimination, Archived deferral, depends_on
  resolution, order-is-next, `effective_max_agents` (repo → workspace →
  unlimited) with queueing-not-rejection, `budget-exhausted` label set/clear,
  claim → InProgress, spawn-failure retries at 1s/5s/30s with per-attempt
  teardown (agent Failed, token removed, budget released), terminal failure →
  task Blocked + TaskBlocked to the repo orchestrator + priority-1
  `ComputeTargetUnavailable` to workspace Owner/Admin members. All domain
  transition edges used exist (task.rs, agent.rs).
- Phase 6: the MCP `gyre_agent_complete` surface now creates the completion MR
  through the same `create_mr_for_completion` core as REST (the audited
  divergence is closed); gate triggering is real.
- Phase 7: gate failures → per-gate GateFailure messages in the agent's inbox
  carrying the gate's real recorded output (persisted via emit_event,
  Destination::Agent), failed MR Closed, same agent entity re-launched with a
  fresh JWT on the same worktree/branch, iteration incremented,
  GYRE_ITERATION injected on all compute backends, repo
  `max_agent_iterations` (default 10) enforced with task Blocked / agent
  Failed / token removed at the ceiling. Budget accounting balanced across
  spawn/complete/re-spawn/teardown.
- Phase 8: gate reviewer fires mechanically via the auto-enqueue
  (AgentReview → run_agent_review_gate); interrogation (30-min JWT,
  read-only) pre-existing and genuine; reconciliation routes meta-spec
  changes to Delegation tasks that now cascade to workers through Phase 4 —
  a documented interpretation (coverage row 11), defensible.
- New `Repository.max_agents`/`max_agent_iterations` columns: migration
  000060 (next unused sequence, portable SQL), SQLite + PG round-trip,
  PUT /repos/:id validation (≤ workspace per-repo limit, ≥ 1) with 400-path
  test.

## Deferred verifier probes (server builds exceed the review-stage budget)

1. `cargo test -p gyre-server --test e2e_ralph_loop` — decisive for F1.
2. `cargo test -p gyre-server --lib ralph_loop`
3. `cargo test -p gyre-server --lib signal_chain`
4. `cargo test -p gyre-server --lib api::repos`
5. `cargo test -p gyre-server --lib api::merge_queue`
6. `cargo test --all` (acceptance criterion)
