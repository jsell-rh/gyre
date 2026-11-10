---
title: "Implement meta-spec rollout policies"
spec_ref: "meta-spec-reconciliation.md §7"
depends_on: [task-156]
progress: complete
coverage_sections:
  - "meta-spec-reconciliation.md §7 Rollout Policy"
commits: ["6b6cbbd87cbd5df5a190e682c1ced6f8057651d5", "9caa8566fc874dbacffb341ddb2af0b086d3f418", "de208c147ff0653404a2b6b0a8d33e1f2e8b91a1", "aa73b69a563fedd2c99929831aca325cfc1d3dc5", "e67c467191726aa1e3510cd15c2ea0ee044362b5", "a1ff51f2cd964fa606e58eee76baaf73435a5be3"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §7 — Rollout Policy:

> Workspace admins configure rollout policies for meta-spec changes:
>
> | Strategy | Behavior |
> |---|---|
> | `immediate` | All repos reconciled simultaneously on approval |
> | `rolling` | Repos reconciled in batches (configurable batch size and delay) |
> | `manual` | Repos reconciled one-at-a-time on admin trigger |
>
> Default: `immediate` for workspaces with ≤10 repos, `rolling` for larger workspaces.

## Implementation Plan

1. **Add RolloutPolicy type** (`crates/gyre-domain/src/meta_spec.rs`):
   ```rust
   pub enum RolloutStrategy {
       Immediate,
       Rolling { batch_size: u32, delay_secs: u64 },
       Manual,
   }

   pub struct RolloutPolicy {
       pub strategy: RolloutStrategy,
       pub auto_strategy_threshold: u32,  // default 10 — auto-select based on repo count
   }
   ```

2. **Store rollout policy per workspace**:
   - Add `rollout_policy` field to workspace or meta-spec set configuration
   - Migration to add `rollout_policy_json` column to workspaces or meta_spec_sets table
   - Default: `{ "strategy": "immediate", "auto_strategy_threshold": 10 }`

3. **Integrate with reconciliation controller** (task-156):
   - When reconciliation is triggered, read the workspace's rollout policy
   - For `immediate`: create all reconciliation tasks at once (current behavior)
   - For `rolling`: create tasks in batches, with a delay between batches (use a scheduled follow-up job)
   - For `manual`: create tasks only for repos explicitly selected by the admin

4. **Add API endpoints for rollout policy**:
   - `GET /api/v1/workspaces/{id}/meta-specs/rollout-policy` — get current policy
   - `PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy` — update policy (Admin only)

5. **Tests**:
   - Unit test: immediate strategy creates all tasks at once
   - Unit test: rolling strategy creates tasks in batches
   - Unit test: manual strategy creates no tasks without explicit trigger
   - Unit test: auto-selection picks immediate for ≤10 repos, rolling for >10

## Acceptance Criteria

- [ ] RolloutPolicy with Immediate, Rolling, Manual strategies
- [ ] Workspace-level rollout policy stored in database
- [ ] Reconciliation controller respects rollout policy
- [ ] Rolling strategy creates tasks in configurable batches with delay
- [ ] API endpoints for get/set rollout policy
- [ ] Default auto-selection based on repo count threshold
- [ ] Tests cover all 3 strategies

## Agent Instructions

- Read task-156 for the reconciliation controller this task integrates with
- Read `crates/gyre-domain/src/meta_spec.rs` for existing MetaSpec types
- Read `crates/gyre-server/src/api/meta_specs.rs` for meta-spec API patterns
- The rolling strategy needs a follow-up mechanism — use the job framework to schedule delayed batch processing
- Hexagonal architecture: RolloutPolicy goes in `gyre-domain`, rollout logic in server

## Shipped

Real, production implementation of `meta-spec-reconciliation.md` §7 Rollout
Policy, plus the review repairs for overlapping rolling waves (commit
`de208c147ff0653404a2b6b0a8d33e1f2e8b91a1`), for the flush-path
delegation flag and merge read-failure fail-open (commit
`9caa8566fc874dbacffb341ddb2af0b086d3f418`), and for the Background-history
rolling-rollout stall (commit `6b6cbbd87cbd5df5a190e682c1ced6f8057651d5`,
which also merges the assignment base `637be7187` — conflict resolved in
`mem.rs` imports only). Full commit list in the frontmatter.

### Spec fidelity

The task excerpt above describes immediate/rolling/manual strategies with
batch delays and repo-count-based auto-selection. The live spec
(`specs/system/meta-spec-reconciliation.md` §7, unchanged since the
original spec commits — not modified by this work) defines
Immediate/Rolling/Background with `max_concurrent_agents`,
`ReconciliationPriority`, and `auto_merge`, defaulting to Rolling / Normal /
auto_merge false, with no repo-count auto-selection. Per the goal rules
(specs are the contract), the implementation follows the live spec. The
acceptance checkboxes above are left unchecked because their wording
describes the excerpt, not the spec; the shipped behavior satisfies the
spec's actual criteria. If the manual/delayed-batch/repo-count semantics are
still wanted, that is a spec amendment request, not a code gap.

### Behavior

- **Domain types** (`crates/gyre-domain/src/meta_spec.rs`): `RolloutPolicy`
  `{ strategy, max_concurrent_agents, priority, auto_merge }`,
  `RolloutStrategy` (`Immediate`/`Rolling`/`Background`), and
  `ReconciliationPriority` (`High`/`Normal`/`Low`), serde wire names
  lowercase. Default per spec: Rolling / batch 5 / Normal / auto_merge
  false. `ReconciliationPriority::task_priority()` maps onto `TaskPriority`
  (High→High, Normal→Medium, Low→Low) — the priority the repo orchestrator
  orders work by.
- **Persistence**: new `RolloutPolicyRepository` port (get/upsert/delete,
  same model as `MetaSpecSetRepository` — opaque JSON per workspace) with
  SQLite + Postgres adapters (`rollout_policies` table, migration
  `2026-11-06-000057_rollout_policies`, portable SQL for both backends) and
  a mem adapter; wired into `AppState` via the `store!` pattern, so a
  restart keeps the policy. An unset policy is the spec default.
- **Controller integration** (`crates/gyre-server/src/reconciliation.rs`):
  `run_reconciliation` loads the workspace's policy and dispatches on the
  strategy. `Immediate` creates every task at once. `Rolling` creates only
  the first `max_concurrent_agents` tasks and defers the remainder
  (kv-backed, restart-safe); the shared `on_reconciliation_task_terminal`
  hook — used by all three terminal-transition surfaces (REST task update,
  REST repo deletion, MCP `update_task`) — creates the next batch when the
  previous batch has no open reconciliation tasks, before the §11
  wave-completion check, so `ReconciliationCompleted` fires exactly once at
  the end of the whole rollout. Every advance re-reads the live policy, so
  a mid-rollout strategy switch flushes the remainder at once and a
  `max_concurrent_agents` change applies to the next batch. `Background`
  creates all tasks without `task_type`: they sit in Backlog and cannot
  trigger agent spawning (enforced by the existing `spawn_agent_core`
  task-type gate) — the repo orchestrator decides when to engage them.
- **Overlapping waves** (review repair, commit `de208c147`): the deferred
  remainder is MERGED, never overwritten. `merge_pending_rollout` retains
  all stored deferred entries except those superseded by an incoming entry
  with the same (repo, title) or consumed by the caller (created as tasks,
  deduplicated away, or resolved out of the loaded snapshot). A second
  meta-spec change approved while a rolling rollout is in flight (a wide
  window by design — Rolling waits for each batch and is the §7 default)
  extends the pending work instead of discarding the first wave's deferred
  tasks; both remainders are batched together and drained by the normal
  advance, with `ReconciliationCompleted` still firing exactly once at the
  very end.
- **auto_merge**: when the policy enables it, a reconciliation MR created
  by agent completion (§6 step 6) is enqueued into the merge queue, which
  triggers gate execution; the merge processor merges a queued entry only
  after all gates pass — gate passage is still required, only the merge
  trigger is automatic. Default false leaves the MR for a human/agent
  enqueue.
- **API**: `GET/PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy`.
  GET returns the stored policy or the spec default. PUT is Admin-only,
  validates strategy/priority names, rejects `max_concurrent_agents < 1`
  (a rolling rollout with batch 0 would never create a task), merges
  omitted fields with current values, emits a `rollout_policy_updated`
  event, and is registered in the ABAC route resolver (`meta_spec`
  resource) — no new exemptions.

### Review repair (attempt 5a60eba0, commit `de208c147`)

Review finding `dde37e8e` reproduced on candidate `1ca46e9e7`: the rolling
strategy's pending remainder lived under a per-workspace singleton KV key
and `persist_pending_rollout` unconditionally `kv_set` it, so a second wave
triggered while a rolling rollout was in flight overwrote the first wave's
deferred tasks — silent work loss (persona tasks 1/4 vs reviewer tasks
4/4 in the reviewer's reproduction). Fixed by replacing the overwrite
persist with `merge_pending_rollout` (see Behavior above); both call sites
(`run_reconciliation`'s rolling branch — passing the first-batch task keys
as consumed — and `maybe_advance_rolling_rollout` — passing the loaded
snapshot keys as consumed) merge instead of overwrite. Two regression
tests were added, each mutation-verified to fail under the semantics it
guards against:

- `overlapping_rolling_waves_retain_both_remainders` — fails under the old
  overwrite semantics (the review reproduction: both waves' deferred
  remainders retained, every repo receives both waves' tasks, completion
  fires exactly once).
- `retriggered_rolling_wave_supersedes_not_duplicates` — fails under a
  pure-append merge (guards the duplication the merge could introduce): a
  same-version re-trigger consumes the deferral of work it just put in
  flight and supersedes the matching deferred entry instead of
  duplicating it. The test identifies repos by observed task state, not id
  order, because mem-adapter repo iteration order is not deterministic
  across processes (an earlier hardcoded-order draft was flaky and fixed
  before commit).

Evidence retained at `/tmp/stage/review-evidence/task-157-overlap-fix-*.txt`:
focused tests, diff-scoped rustfmt/clippy gates, and a notes file recording
the mutation checks (temporary source mutations, reverted afterwards).

### Review repair (attempt fd31a1cb, commit `9caa8566f`)

Two findings on candidate `c2904d4f6`, both fixed:

- **F1 — flush-path delegation flag**: `maybe_advance_rolling_rollout`'s
  flush path hardcoded `delegation = true`, so a mid-rollout switch to the
  Background strategy created the deferred remainder with
  `task_type: Some(Delegation)` — those tasks can trigger agent spawning,
  contradicting §7 Background ("leave them in Backlog... repo orchestrator
  decides priority") and disagreeing with `run_reconciliation`'s own
  Background branch. Fixed by deriving the flag from the re-read live
  policy (`policy.strategy != Background`); a Background switch now
  flushes inert Backlog tasks with no task_type.
- **F2 — merge fail-open on read failure**: `merge_pending_rollout`
  failed open on a `kv_get` Err — it warned "keeping only the incoming
  remainder" and then wrote that incoming-only list, overwriting the
  stored deferred remainder behind a transient read failure (the same
  silent-work-loss class the `de208c147` repair fixed). Fixed by aborting
  the merge on read failure: the function now returns `Option<usize>`
  (`None` = aborted, nothing written, stored state untouched), and both
  callers log the abort operator-visibly. An aborted advance self-heals:
  already-created tasks are deduplicated on the next advance. The
  unparseable-JSON arm remains a deliberate recovery (garbage cannot be
  merged with; the incoming remainder keeps the rollout drainable).

Two regression tests added, each mutation-verified to fail under the
semantics it guards against:

- `rolling_strategy_switch_to_background_flushes_inert_tasks` — fails
  when the flush path hardcodes delegation=true (asserts flushed tasks
  sit in Backlog with no task_type after a mid-rollout Background
  switch).
- `merge_pending_rollout_read_failure_leaves_stored_state_untouched` —
  fails under the old fail-open semantics (a `FlakyReadKvStore` test
  double injects a kv read failure during a second wave; the stored
  remainder must remain exactly the first wave's deferrals and still
  drive the next batch afterwards). Supported by a new test-only
  `test_state_with_kv_store` helper in `mem.rs`.

Evidence retained at
`/tmp/stage/review-evidence/task-157-fd31a1cb-repair.txt`.

### Review repair (attempt c9dc5a11, commit `6b6cbbd8`)

Review finding `c9dc5a11` reproduced on candidate `fe7beea54`:
`maybe_advance_rolling_rollout`'s next-batch gate counted ANY non-terminal
reconciliation-labeled task as "batch in flight", but §7 Background tasks
are created without a `task_type` and sit in Backlog by design until the
repo orchestrator engages them (potentially forever) — so a workspace with
Background history stalled a later Rolling rollout indefinitely: batch 1
completed, the advance saw the inert Backlog tasks as in-flight work, and
no next batch was ever created (and the strategy-switch flush, living
inside the same never-running advance, was unreachable too). Fixed by
gating the advance on the rollout's own in-flight work — non-terminal
reconciliation tasks carrying a `task_type` (the spawning signal) — which
is internally consistent with `run_reconciliation`, which starts a new
wave's first batch without any in-flight check.

Regression test `background_history_does_not_stall_rolling_rollout`
reproduces the review scenario end-to-end (Background wave parks 3 inert
tasks → policy switched to Rolling → new wave creates batch 1 + 2 deferred
→ batch completions must keep advancing the rollout to the very end, with
the parked tasks untouched) and was mutation-verified to fail under the
old gate exactly at the reviewer's reproduction point ("batch 2 must be
created after batch 1 completes despite Background history: left 0,
right 1"). A test-only `finish_active_reconciliation_tasks` helper drives
only tasks carrying a `task_type` to Done, leaving inert Background tasks
parked — `finish_all_reconciliation_tasks` would have finished them too
and silently un-stalled the scenario.

Evidence retained at
`/tmp/stage/review-evidence/task-157-c9dc5a11-repair.txt`.

### Test evidence

Focused probes at head `6b6cbbd8`:

- `cargo test -p gyre-server --lib reconciliation::tests` — 33 passed
  (28 pre-existing + 5 regression tests from the three repairs), covering:
  immediate creates all tasks at once; rolling creates batch 1 only and
  advances on completion; background leaves tasks in Backlog; priority
  mapping; strategy-switch flush; strategy-switch to Background flushes
  inert tasks; batch-size change applies to the next batch; overlapping
  waves retain both remainders; re-triggered wave supersedes not
  duplicates; pending-state read failure leaves stored state untouched;
  background history does not stall a rolling rollout; auto_merge enqueues
  / default does not / non-reconciliation MRs ignored; unset policy is the
  spec default; `ReconciliationCompleted` fires exactly once at rollout
  end. Suite run three times consecutively — stable (no order-dependent
  flakiness).
- `cargo test -p gyre-server --lib api::meta_specs` — 25 passed.
- Diff-scoped gates against base `637be7187`:
  `check-rustfmt-diff.py` → "changed lines clean (20 Rust files checked)";
  `check-clippy-diff.py` → "changed lines clean (20 Rust files, 1094
  existing warnings outside changes)".

Full workspace suites, architecture checks, all-target Clippy, and GitHub
CI are owned by verification/publication and were not repeated here.
