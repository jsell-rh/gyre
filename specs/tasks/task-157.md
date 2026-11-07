---
title: "Implement meta-spec rollout policies"
spec_ref: "meta-spec-reconciliation.md §7"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §7 Rollout Policy"
commits: ["e67c467191726aa1e3510cd15c2ea0ee044362b5", "a1ff51f2cd964fa606e58eee76baaf73435a5be3"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §7 — Rollout Policy (the live spec text
in `specs/system/meta-spec-reconciliation.md` is the contract; the original
task excerpt's immediate/rolling/manual + repo-count auto-selection table
does not match the spec and was not implemented — see Shipped, Spec
fidelity):

> Each workspace has a rollout policy that governs how reconciliation proceeds:
>
> ```rust
> pub struct RolloutPolicy {
>     pub strategy: RolloutStrategy,
>     pub max_concurrent_agents: u32,
>     pub priority: ReconciliationPriority,
>     pub auto_merge: bool,
> }
> ```
>
> - `Immediate` — create all reconciliation tasks immediately; budget limits still apply.
> - `Rolling` — create tasks in batches; wait for each batch to complete before
>   starting the next; batch size = `max_concurrent_agents`.
> - `Background` — create tasks but leave them in Backlog; reconciliation
>   competes with feature work for agent time; repo orchestrator decides priority.
>
> `ReconciliationPriority`: High (before feature work) / Normal (interleaved) /
> Low (only when no feature work is pending).
>
> Default: `Rolling` strategy, `Normal` priority, `auto_merge: false` (MRs
> require gate passage but merge automatically if gates pass).

## Acceptance Criteria

- [x] RolloutPolicy with Immediate, Rolling, Background strategies
- [x] Workspace-level rollout policy stored in database
- [x] Reconciliation controller respects rollout policy
- [x] Rolling strategy creates tasks in configurable batches, advancing when each batch completes
- [x] API endpoints for get/set rollout policy
- [x] Default policy per spec (Rolling / Normal / auto_merge false)
- [x] Tests cover all 3 strategies

## Shipped

Real, production implementation of `meta-spec-reconciliation.md` §7 in commit
`a1ff51f2cd964fa606e58eee76baaf73435a5be3`.

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
  restart keeps the policy.
- **Controller integration** (`crates/gyre-server/src/reconciliation.rs`):
  `run_reconciliation` loads the workspace's policy and dispatches on the
  strategy. `Immediate` creates every task at once (current behavior).
  `Rolling` creates only the first `max_concurrent_agents` tasks and
  persists the remainder (kv-backed, restart-safe); the shared
  `on_reconciliation_task_terminal` hook — now used by all three
  terminal-transition surfaces (REST task update, REST repo deletion, MCP
  `update_task`) — creates the next batch when the previous batch has no
  open reconciliation tasks, before the §11 wave-completion check, so
  `ReconciliationCompleted` fires exactly once at the end of the whole
  rollout. Every advance re-reads the live policy, so a mid-rollout
  strategy switch flushes the remainder at once and a
  `max_concurrent_agents` change applies to the next batch (the admin's
  latest configuration governs). `Background` creates all
  tasks without `task_type`: they sit in Backlog and cannot trigger agent
  spawning (enforced by the existing `spawn_agent_core` task-type gate) —
  the repo orchestrator decides when to engage them.
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

### Spec fidelity

The original task excerpt described immediate/rolling/manual strategies
with batch delays and repo-count-based auto-selection. The live spec
(`specs/system/meta-spec-reconciliation.md` §7) defines
Immediate/Rolling/Background with `max_concurrent_agents`,
`ReconciliationPriority`, and `auto_merge`, defaulting to Rolling/Normal/
auto_merge false with no repo-count auto-selection. Per the goal rules
(specs are the contract), the implementation follows the live spec. If the
manual/delayed-batch semantics are still wanted, that is a spec amendment
request, not a code gap.

### Test evidence

Focused probes (full output retained at
`/tmp/stage/review-evidence/task-157-probes.txt` on the assignment
sandbox; product commit `a1ff51f2cd964fa606e58eee76baaf73435a5be3`):

- `cargo test -p gyre-domain --lib meta_spec::` — 4 passed: spec default,
  wire-name round-trips, priority mapping, full JSON shape.
- `cargo test -p gyre-adapters rollout` — 5 passed: SQLite get/upsert/
  overwrite/delete/missing + domain round-trip through the stored row.
- `cargo test -p gyre-server --lib reconciliation::` — 27 passed,
  including: immediate creates all tasks at once; rolling creates batch 1
  only, does not advance while the batch is open, advances on completion,
  fires `ReconciliationCompleted` exactly once at rollout end, clears
  pending state; background leaves tasks in Backlog without task_type;
  priority mapping; strategy-switch flush; batch-size change applies to
  the next batch; auto_merge enqueues / default does not /
  non-reconciliation MRs ignored; unset policy is the spec default.
  Pre-existing §6/§10/§11 tests still green.
- `cargo test -p gyre-server --lib api::meta_specs::` — 24 passed,
  including GET default, PUT round-trip, partial update, Admin-only 403,
  invalid strategy/priority/batch-size 400, unknown workspace 404.
- Terminal-hook refactor regression suites: `api::tasks::` (6),
  `api::repos::` (16), `api::spawn::` (34), `mcp::` (78) — all passed.
- Mechanical checks pass with no new exemptions: check-arch,
  check-abac-route-registry, check-migration-versions,
  check-migration-sql-portability, check-in-memory-state-stores,
  check-unwired-port-methods, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-mem-port-contracts,
  check-inert-enforcement, check-forged-scope-fields,
  check-lossy-secret-conversion, check-dead-message-kinds,
  check-byte-slice-truncation, check-relative-path-defaults,
  check-forwarded-header-trust.

Full workspace suites, all-target Clippy, and GitHub CI are owned by
verification/publication and were not repeated here.
