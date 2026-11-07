---
title: "Implement meta-spec rollout policies"
spec_ref: "meta-spec-reconciliation.md §7"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §7 Rollout Policy"
commits: ["a1ff51f2cd964fa606e58eee76baaf73435a5be3", "e67c467191726aa1e3510cd15c2ea0ee044362b5"]
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
Policy in commits `a1ff51f2cd964fa606e58eee76baaf73435a5be3` and
`e67c467191726aa1e3510cd15c2ea0ee044362b5`, re-verified after merging the
assignment base `7da7bbfff69da433592962a911abe23677d5ac3a` (the merge kept
the rollout implementation byte-identical; only `api/meta_specs.rs`
combined it with base's task-077 per-handler admin gates).

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
  the first `max_concurrent_agents` tasks and persists the remainder
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

### Test evidence

Focused probes re-run post-merge on this branch (head `3025e601a`, output
retained at `/tmp/stage/review-evidence/task-157-probes.txt`):

- `cargo test -p gyre-domain meta_spec` — 6 passed: spec default,
  wire-name round-trips, priority mapping/round-trip, full JSON shape.
- `cargo test -p gyre-server --lib reconciliation::tests` — 28 passed,
  including: immediate creates all tasks at once; rolling creates batch 1
  only and advances on completion; background leaves tasks in Backlog;
  priority mapping; strategy-switch flush; batch-size change applies to
  the next batch; auto_merge enqueues / default does not /
  non-reconciliation MRs ignored; unset policy is the spec default;
  `ReconciliationCompleted` fires exactly once at rollout end. Pre-existing
  §6/§10/§11 tests still green.
- `cargo test -p gyre-server --lib api::meta_specs` — 25 passed, including
  GET default, PUT round-trip, partial update, Admin-only 403, invalid
  strategy/priority/batch-size 400, unknown workspace 404.
- Rollout implementation files (domain `meta_spec.rs`, `reconciliation.rs`,
  sqlite `rollout_policy.rs`, migration 000057, `gyre-ports`) verified
  byte-identical to candidate `1e76c52eb` after the merge
  (`task-157-rollout-files-vs-candidate.diff`, empty).

Full workspace suites, architecture checks, all-target Clippy, and GitHub
CI are owned by verification/publication and were not repeated here.
