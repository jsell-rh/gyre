# Review — task-118: Implement agent spawning, implementation phase & Ralph loop

Round: 2 (round 1 reviewed candidate b14eb059's implementation; this round
re-reviewed the repaired candidate).
Reviewer scope: `git diff 42d894f8..c7de97dc` (commits 26edb8fc3 implementation,
6ae1e989c review-repair, 49fa45ef3/c7de97dc1 docs/attribution) against
`specs/system/agent-runtime.md` §1 Phases 4–8 and the task contract.
Evidence: `/tmp/stage/review-evidence/` (review-summary.md, candidate.diff,
cargo-check-gyre-server.log, probe-release-claim-route.{rs,out}).

## Verdict (round 2): NOT approved — one material defect

The round-1 findings are properly repaired (verified in source, see below), and
the implementation is real end-to-end (scheduler mechanics, MCP completion MR,
auto-gates, Ralph-loop re-spawn/max-iterations/TaskBlocked routing, spawn-failure
retry + priority-1 notification, Phase 8 wiring). `cargo check -p gyre-server`
is clean; gyre-common/domain/ports/adapters tests pass (595 + 439); all
mechanical invariant scripts pass. But a new material defect was found on a
reachable path, proven by a domain-level probe:

### F1 (material): `release_claim` can never release a worker claim — budget-race path strands the task InProgress forever

`crates/gyre-server/src/signal_chain.rs` `release_claim` routes
InProgress→Review→InProgress→Backlog, but the domain transition table
(`gyre-domain/src/task.rs:126`) has **no edge into Backlog at all** — the route
nets back to InProgress, the final transition is always rejected, and the
function returns without persisting anything. Probed with a scratch crate
replicating the exact sequence: final status is always `InProgress`
(`/tmp/stage/review-evidence/probe-release-claim-route.out`).

Reached when `try_spawn_worker` returns `WorkerSpawnOutcome::Blocked` — i.e.
`spawn_agent_core` answers `ApiError::TooManyRequests` because the workspace
budget was exhausted *after* the scheduler's own pre-check passed (a concurrent
REST spawn consuming the last concurrent-agent slot inside the cycle's await
window, or budget exhaustion during the 1s/5s/30s retry-backoff window — the
background retry re-attempts without a budget pre-check). The task is then
stranded `InProgress` permanently:

- `spawn_implementation_workers` scans only `Backlog`;
- the stale-InProgress recovery in `scheduler_run_once` re-claims only
  Delegation/Coordination — Implementation tasks are skipped by the task_type
  filter, so the "stale re-claim will retry" comment is false for workers
  (server crash mid-retry strands the task the same way);
- no API can return a task to Backlog (`POST /tasks/:id/transition` uses the
  same domain table).

This violates agent-runtime.md §1 Phase 4 step 3 — "task stays `Backlog` with a
`BudgetExhausted` flag" — on this path: the task stays InProgress with no flag,
no retry, no notification, and no recovery short of direct storage surgery. No
test covers the Blocked/release path (`scheduler_flags_budget_exhausted` covers
only the pre-check path), which is why the suite stays green. Fix direction:
either add a domain edge for releasing a worker claim (e.g. InProgress→Backlog
or route via Blocked and teach `spawn_implementation_workers` to re-scan Blocked
Implementation tasks whose label/flag indicates a retryable release), persist
the BudgetExhausted flag on that path, and add a test that drives
`try_spawn_worker` into the `Blocked` outcome and asserts the task is re-scanned
by a later cycle.

### F2 (minor): fixed byte-index slice in `8.min()` disguise

`try_spawn_worker` computes `&task.id.as_str()[..8.min(task.id.as_str().len())]`
— a fixed byte-index slice that evades `check-byte-slice-truncation.sh` (the
`8.min()` form is not a literal match). Not triggerable today (task ids are
server-generated UUIDs, always ASCII), but it is exactly the class the repo
invariant forbids ("Never slice strings at fixed byte indexes"). Truncate on a
char boundary or use the id directly.

## Round-1 findings — repair verification (all three confirmed fixed)

1. **Duplicate merge-queue entry / flaky e2e** — `enqueue_core` is idempotent
   per open MR (returns the existing non-terminal entry, 200 OK; terminal
   entries allow a fresh retry enqueue; gates not re-triggered for an existing
   entry). Tests `enqueue_is_idempotent_per_open_mr` and
   `enqueue_after_terminal_entry_creates_fresh_entry` bite; `e2e_ralph_loop`
   step 10 no longer manually re-enqueues and asserts no duplicates.
2. **tokio `test-util` in the workspace production feature list** — moved to a
   gyre-server dev-dependency re-enable; production builds no longer compile
   tokio test-only code.
3. **Stale-snapshot whole-row claim persist** — `spawn_implementation_workers`
   re-reads the freshest task row before the claim (and before the
   budget-exhausted label write) and only claims a still-Backlog Implementation
   row; the same defense as the orchestrator loop.

## Probes run (review sandbox; 4 GiB cgroup)

- `cargo check -p gyre-server` — clean (log retained).
- `cargo test -p gyre-common -p gyre-domain -p gyre-ports` — 595 passed.
- `cargo test -p gyre-adapters` — 439 passed.
- All mechanical invariant scripts (arch, migrations, dead message kinds, MCP
  write tools, mem port contracts, in-memory stores, unbounded HTTP, ABAC
  registry/exemptions, forged scope fields, inert enforcement, scope defaults,
  lossy secrets, forwarded headers, fail-open refs, relative paths, SQL
  portability, byte-slice truncation, commit attribution) — OK. The
  commit-attribution failure against the working tree is the pipeline's
  unstaged reset of `specs/tasks/task-118.md` to `commits: []`; the committed
  candidate HEAD passes.
- Domain probe proving F1 (see above).

## Deferred to the independent verifier

gyre-server code generation exceeds the review sandbox budget (wrapper-enforced);
the verifier must run:

```
cargo test -p gyre-server --lib ralph_loop
cargo test -p gyre-server --lib signal_chain
cargo test -p gyre-server --lib api::merge_queue
cargo test -p gyre-server --lib api::spawn
cargo test -p gyre-server --lib api::repos
cargo test -p gyre-server --lib mcp
cargo test -p gyre-server --test e2e_ralph_loop
cargo test --all        # task acceptance criterion
```

Note: `worker_spawn_failure_retries_then_blocks` relies on `kubectl` being
absent/unreachable (deterministic in CI; a developer machine with a live kubectl
context could make the launch succeed and the test fail).

## Non-blocking observations

- Double-completion is guarded on both surfaces (MCP fails Idle→Idle before MR
  creation; REST returns early on `created=false`), so no double Ralph-loop
  watcher.
- `GYRE_ITERATION` is injected through the shared `spawn_config.env` and thus
  reaches every compute backend.
- The ralph_loop unit tests drive `on_gates_failed`/`complete_task_on_merge`
  directly (no watcher timing dependence) and assert real state (agent
  re-activated, iteration, MR closed, inbox message carrying the gate's real
  recorded output, token lifecycle) — not self-confirming.
