# Review — task-118: Implement agent spawning, implementation phase & Ralph loop

Round: 5 (round-4 verification repair re-review).
Reviewer scope: `git diff fbf1a63b..936cea925` (full candidate). Evidence at
`/tmp/stage/review-evidence/probe-log.md` and
`/tmp/stage/review-evidence/network-restriction.txt`.

## Verdict

Approved. No code defects found. Rust test execution was blocked by a
sandbox transport restriction (crates.io unreachable, no target cache) —
the deferred probes the verifier must run are recorded below and in the
evidence directory; the restriction is infrastructure, not a code defect.

## What was reviewed (independent)

- **Phase 4 scheduler** (`signal_chain.rs`): `spawn_implementation_workers`
  runs inside `scheduler_run_once` under the cycle lock (30s loop started
  in `main.rs`). Verified against the spec line by line: `task_type:
  Implementation` discriminator (untyped/Delegation/Coordination excluded),
  `depends_on` satisfied (Done/Cancelled; deleted deps resolved), `order`
  next within the decomposition group, repo-not-Archived (queues, not
  rejects), `max_agents` (repo override → workspace fallback → unlimited;
  queues, never 429s), workspace budget (task flagged `budget-exhausted`,
  stays Backlog), fresh-row re-read before the Backlog→InProgress claim
  (no lost-update on concurrent API mutation), spawn through the same
  `spawn_agent_core` the REST surface uses, stale-claim crash recovery via
  the domain's dedicated `release_claim` edge. All queries go through the
  real ports (tasks/repos/agents/worktrees/budget).
- **Phase 5/6**: both completion surfaces (REST `spawn.rs`, MCP `mcp.rs`)
  create the MR via `create_mr_for_completion` (idempotent per
  agent+branch, jj bookmark-set + git export, spec_ref propagation) and
  call `on_agent_mr_created` → `merge_queue::enqueue_core` (idempotent per
  open MR, terminal entries allow retry) → `trigger_gates_for_mr`. Token
  revocation + budget-slot release mirror each other on both surfaces.
- **Phase 7**: bounded gate watcher (20-min deadline) → pass path (merge
  watch, task Done via the valid Review→Done edge, agent already
  Idle/token-revoked at completion) / fail path (one GateFailure message
  per failed gate carrying the gate's real recorded output into the
  agent's inbox, failed MR closed, iteration++, ceiling check against
  repo `max_agent_iterations` → default 10). Max iterations: task
  Blocked, agent Failed, JWT removed, `TaskBlocked` routed to the repo
  orchestrator's inbox (orchestrator spawned on demand; workspace-tier
  fallback). Re-spawn: same agent entity/persona/meta-spec set/worktree,
  fresh JWT + fresh process. Spawn-failure ladder 1s/5s/30s
  (`SPAWN_RETRY_BACKOFF_SECS`) on BOTH first-spawn and re-spawn paths,
  with claim-touching to keep stale-claim recovery from racing; terminal
  state adds the priority-1 `ComputeTargetUnavailable` notification to
  workspace Owner/Admins. Spec rejection (`api/specs.rs`) cancels
  in-flight tasks by spec path, stops Active agents, revokes task-scoped
  secrets, and sends the shutdown message with a 60s grace field.
- **Phase 8**: interrogation agents (30-min JWT TTL), gate reviewer
  (AgentReview gate with scoped reviewer identity), reconciliation on
  meta-spec set change — pre-existing mechanisms correctly reused; the
  re-spawn path resolves the workspace compute target per §8.
- **Supporting changes**: per-repo `max_agents`/`max_agent_iterations`
  with PUT validation (cannot exceed workspace limit; >= 1), portable
  migration `2026-11-12-000060_repo_agent_limits` (nullable ADD COLUMN,
  runs on both SQLite and PG), sqlite + postgres adapter columns,
  `TaskBlocked` message kind (payload schema registered, emitter exists —
  dead-message-kind check green), `ComputeTargetUnavailable`
  priority-1 notification type.

## Round-4 verification repair (`56a528d05`) — independently confirmed sound

The prior verification (attempt 055fbdf3, pre-repair ancestor 6c12bf402)
failed 4 gyre-server lib tests. The repair's root-cause account checks
out on every point:

- The port contract is real and pre-existing: `list_by_workspace`
  excludes Directed messages in BOTH adapters (`sqlite/message.rs`
  `.filter(messages::to_type.ne("agent"))`, `mem.rs`
  `!matches!(&m.to, Destination::Agent(_))`), present verbatim in the
  assigned base fbf1a63b — not introduced to make tests pass.
- `notify_task_blocked` production routing (orchestrator inbox when one
  can be ensured, workspace tier otherwise) is the spec's primary
  routing; the old assertions used an accessor that structurally cannot
  observe it.
- Assertion strength preserved: `max_iterations_blocks_task_and_fails_agent`
  still asserts Blocked/Failed/iteration/token-revoked AND a precise
  orchestrator-inbox delivery check; `collect_task_blocked_messages`
  returns `[]` if production stops emitting TaskBlocked (not
  tautological); `respawn_failure_retries_then_blocks_task` proves the
  full backoff ladder via paused-clock elapsed ≥ 36s.
- The dep-walk fix matches the domain transition table (InProgress→
  Review→Done; direct InProgress→Done is the exact InvalidTransition the
  failing run reported).
- The repair touches only `#[cfg(test)]` code — no production behavior
  was altered to make the suite green.
- All four prior failure panics occurred after the status assertions
  passed, at the message-tier assertions — consistent with this account.

## Checks run

All 19 mechanical enforcement scripts pass on the candidate (migration
versions, arch, dead-message-kinds, byte-slice, in-memory stores, inert
enforcement, ABAC registry + exempt handlers, MCP write tools, migration
SQL portability, forged scope fields, forwarded-header trust,
unbounded HTTP, mem port contracts, fabricated scope defaults, lossy
secrets, scope literals, fail-open refs, relative paths, task-commit
attribution against the committed HEAD). No exemption-file entries were
added relative to base.

## Deferred to the verifier (transport restriction: crates.io unreachable)

1. `cargo test --all` — final acceptance criterion. Prior run failed
   only the 4 tests addressed by the verified-sound test-tier repair;
   1747 others passed and the repair touches no production code.
2. Focused re-run: `cargo test -p gyre-server --lib` with filters
   `ralph_loop::tests::max_iterations_blocks_task_and_fails_agent`,
   `ralph_loop::tests::respawn_failure_retries_then_blocks_task`,
   `signal_chain::tests::stale_worker_claim_released_and_respawned`,
   `signal_chain::tests::worker_spawn_failure_retries_then_blocks`.
3. `cargo test -p gyre-server --test e2e_ralph_loop`.
4. `cargo check -p gyre-server --tests` (cfg(test) helper compiles).

No web/ files changed in this diff, so no frontend probe is required.
