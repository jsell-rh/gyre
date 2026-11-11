# Review — task-118: Implement agent spawning, implementation phase & Ralph loop

Round: 3 (re-review of repair commit `73f302ca` against the round-2 durable
findings F1–F3; F4 is a standing verifier item).
Reviewer scope: `git diff cfeefbda..6c12bf402` (23 commits; implementation
`26edb8fc3` + repair rounds `6ae1e989c`, `a08ef0e79`, `c8b4e5efc`,
`096cd9bd6`, `73f302cae`). Evidence:
`/tmp/stage/review-evidence/evidence.md` and per-probe logs.

## Round 3 verdict

All three round-2 code findings are repaired with real production behavior
and behavior-asserting tests; no new defects found in the repaired paths or
the surrounding Phase 4–8 machinery.

- **F1 (major, re-spawn spawn-failure policy) — fixed.**
  `ralph_loop::respawn_with_retry` applies the full Phase 7 ladder to the
  re-spawn path: 3 retries at the shared `SPAWN_RETRY_BACKOFF_SECS`
  [1,5,30] with `touch_task` keeping the claim fresh; terminal state
  blocks the task from the re-read freshest row, fails the agent, removes
  the freshly-minted re-spawn JWT from `agent_tokens` (agent JWT auth
  validates presence + signature, not status), emits `TaskBlocked` to the
  repo orchestrator, and creates the priority-1
  `ComputeTargetUnavailable` notification for workspace Owner/Admin
  members via the shared `notify_compute_target_unavailable`.
  `respawn_failure_retries_then_blocks_task` (paused clock) asserts the
  full 36s ladder ran — the pre-fix single-attempt behavior fails the
  elapsed assertion — plus JWT absence, priority-1 notification, and
  TaskBlocked routing.
- **F2 (minor, no crash recovery for claimed Implementation tasks) —
  fixed.** `recover_stale_worker_claims` (head of
  `spawn_implementation_workers`, under the scheduler cycle lock)
  releases stale (≥60s) InProgress Implementation claims whose assigned
  agent is not Active, through the domain's only InProgress→Backlog edge
  `Task::release_claim`; an Active agent's claim is never released
  (covered by `stale_worker_claim_with_active_agent_is_kept`), and the
  release → later-cycle re-spawn with a fresh worker is covered by
  `stale_worker_claim_released_and_respawned`.
- **F3 (minor, env pollution in test) — fixed.** GYRE_AGENT_COMMAND is
  pinned behind an `EnvVarRemove` Drop guard that also removes it on
  panic unwind.
- **F4 (verification) — carried to the verifier.** Server test execution
  exceeds the review-stage build budget. This review confirmed
  `cargo check -p gyre-server --tests` exits 0 (all test code compiles)
  and ran `cargo test -p gyre-domain` (466+8 passed) and `-p gyre-common`
  (124 passed). The independent verifier MUST run: `cargo test --all`;
  `cargo test -p gyre-server ralph_loop`; `cargo test -p gyre-server
  signal_chain`; `cargo test -p gyre-server merge_queue`; `cargo test -p
  gyre-server update_repo_agent_limits_validated`; `cargo test -p
  gyre-server --test e2e_ralph_loop`. Five CI scripts
  (self-confirming-tests, assertionless-tests, notification-priority,
  notification-audience, verification-completeness) cannot execute under
  this sandbox's mawk (gawk-only `match(..., m)` syntax); identical
  failures were reproduced on a pristine archive of the base commit, so
  they are environmental — verifier should run them with gawk.

Independent checks run: migration-versions, migration-sql-portability,
arch, dead-message-kinds, inert-enforcement, mem-port-contracts,
in-memory-state-stores, unbounded-external-http, fabricated-scope-defaults,
scope-literal-defaults, lossy-secret-conversion, abac-route-registry,
abac-exempt-handlers, mcp-write-tools, forwarded-header-trust,
forged-scope-fields, fail-open-ref-resolution, byte-slice-truncation,
relative-path-defaults, conditional-test-guards, stub-evaluation,
placeholder-stubs — all pass. Task-commit attribution passes at the
candidate commit (the working-tree failure is the executor's assignment
reset of `specs/tasks/task-118.md`, not the candidate).

Phase 4–8 contract verified by source read (details in evidence.md):
mechanical Backlog+Implementation detection with ordering/preconditions and
queue-not-reject semantics; completion → MR → auto-enqueue → gates on both
REST and MCP surfaces; Ralph loop fail path with real gate output in the
agent inbox, fresh-context re-spawn, per-repo `max_agent_iterations`
(default 10); spawn-failure retry + priority-1 notification on both the
first-spawn and re-spawn ladders; spec-rejection cancellation orders task
cancel before agent stop so cancelled tasks are never requeued.

Non-blocking observation for the spec owner: a manual `stop_agent` leaves
the task InProgress, so the new stale-claim recovery requeues and re-spawns
it after ~60s; the operator-level hold is task cancel/block. The spec does
not define task handling on manual stop — recorded as a judgment call, not
a defect.

## Verdict

`progress: complete` candidate `6c12bf402`. Approved with the F4 server-test
execution list above delegated to the independent verifier.
