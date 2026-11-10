---
title: "Complete analytics event schema and auto-emitted events coverage"
spec_ref: "analytics.md §Event Schema"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "analytics.md §Purpose"
  - "analytics.md §Event Schema"
  - "analytics.md §Auto-Emitted Events"
  - "analytics.md §Query API"
  - "analytics.md §Query Parameters"
commits: ["d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06", "3bcd6f525fbd22ba7e2c60a20ba07c99b9399836"]
---

## Spec Excerpt

From `analytics.md` §Event Schema:

```rust
pub struct AnalyticsEvent {
    pub id: Id,
    pub event_name: String,
    pub agent_id: Option<Id>,
    pub user_id: Option<Id>,
    pub session_id: Option<String>,
    pub workspace_id: Option<Id>,
    pub repo_id: Option<Id>,
    pub properties: serde_json::Value,
    pub timestamp: u64,
}
```

### Auto-Emitted Events (spec-required)

| Event Name | Trigger | Properties |
|---|---|---|
| `task.status_changed` | Task transitions status | `old_status`, `new_status`, `task_id`, `assigned_to` |
| `mr.merged` | MR successfully merged | `mr_id`, `repo_id`, `gate_count`, `queue_wait_secs` |
| `mr.closed` | MR closed without merge | `mr_id`, `repo_id`, `reason` |
| `agent.spawned` | Agent spawned | `agent_id`, `task_id`, `compute_target`, `persona` |
| `agent.completed` | Agent completes successfully | `agent_id`, `task_id`, `duration_secs` |
| `agent.failed` | Agent fails or is killed | `agent_id`, `task_id`, `reason` |
| `merge_queue.processed` | Queue entry processed | `mr_id`, `outcome`, `wait_secs` |
| `gate.failed` | Quality gate fails | `gate_id`, `gate_type`, `mr_id`, `output_snippet` |
| `gate.passed` | Quality gate passes | `gate_id`, `gate_type`, `mr_id`, `duration_secs` |
| `spec.approved` | Spec approved | `spec_path`, `approver_type`, `approval_mode` |
| `budget.warning` | Budget threshold crossed | `workspace_id`, `metric`, `threshold_pct` |
| `search.query` | Full-text search executed | `query_length`, `entity_types`, `result_count`, `duration_ms` |

## Implementation Plan

**NOTE:** The analytics system is partially implemented. The event schema exists, query endpoints exist (`POST/GET /api/v1/analytics/events`, `GET /api/v1/analytics/count`, `GET /api/v1/analytics/daily`), and some auto-emitted events exist (task.status_changed, agent.spawned, mr.merged, merge_queue.processed). This task completes the missing pieces.

1. **Audit existing AnalyticsEvent struct** — verify it matches the spec schema. Add any missing fields (`user_id`, `session_id`, `workspace_id`, `repo_id`) if not present.

2. **Add missing auto-emitted events** — these events are NOT currently auto-emitted and need to be added:

   a. `mr.closed` — in `crates/gyre-server/src/api/merge_requests.rs`, where MRs are closed without merge
   b. `agent.completed` — in the agent completion handler (spawn.rs or agent lifecycle)
   c. `agent.failed` — in the agent failure/kill handler
   d. `gate.failed` / `gate.passed` — in the merge processor gate evaluation loop
   e. `spec.approved` — in `crates/gyre-server/src/api/specs.rs` approval handler
   f. `budget.warning` — in the budget check/enforcement path (spawn.rs budget validation)
   g. `search.query` — in the search endpoint handler

3. **Enrich existing auto-emitted events** — ensure existing events include all spec-required properties:
   - `task.status_changed`: verify `old_status`, `new_status`, `assigned_to` are all present
   - `agent.spawned`: verify `compute_target`, `persona` properties
   - `mr.merged`: verify `gate_count`, `queue_wait_secs` properties
   - `merge_queue.processed`: verify `outcome`, `wait_secs` properties

4. **Verify query API completeness** — the spec requires filtering by `event_name`, `agent_id`, `workspace_id`, `repo_id`, `since`, `until`, `limit`, `group_by`. Verify existing query endpoints support all parameters.

5. **Tests:**
   - Integration test for each new auto-emitted event: trigger the action, verify the event is recorded with correct properties
   - Unit test: AnalyticsEvent struct matches spec schema

## Acceptance Criteria

- [ ] AnalyticsEvent struct matches spec schema (all fields present)
- [ ] All 12 auto-emitted events are recorded at their trigger points
- [ ] Each event includes all spec-required properties
- [ ] Query API supports all spec-required filter parameters
- [ ] Tests pass for each auto-emitted event

## Agent Instructions

- Read `crates/gyre-server/src/api/analytics.rs` for existing analytics implementation
- Read `crates/gyre-server/src/api/mod.rs` lines 430-438 for existing analytics routes
- Grep for `analytics.record` to find all existing auto-emit call sites
- Read `crates/gyre-server/src/api/spawn.rs` for agent lifecycle events
- Read `crates/gyre-server/src/merge_processor.rs` for merge/gate events
- Read `crates/gyre-server/src/api/specs.rs` for spec approval events
- Read `crates/gyre-server/src/api/search.rs` for search endpoint
- Do NOT create new endpoints — the query API routes already exist. Only add missing auto-emitted events and verify completeness.

## Shipped

### Analytics event schema and auto-emitted events

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields
  (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`,
  `repo_id`, `properties`, `timestamp`); `with_scope` populates the
  user/session/workspace/repo scope fields. A serde round-trip test pins
  the shape against the spec schema.
- All 12 spec-required auto-emitted events record at their real trigger
  points with all spec-required properties, each proven by a passing
  per-event test: `task.status_changed`, `mr.merged`, `mr.closed`,
  `agent.spawned`, `agent.completed`, `agent.failed`,
  `merge_queue.processed`, `gate.failed`, `gate.passed`, `spec.approved`,
  `budget.warning`, `search.query`.
- Query API: `POST/GET /api/v1/analytics/events`, `GET /count`, `GET /daily`
  with the full spec filter set — `event_name` (exact or trailing-`*`
  prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`,
  `until`, `limit` (default 100, cap 10_000), `group_by` — implemented on
  SQLite, Postgres, and mem adapters (adapter test
  `analytics_query_filtered_all_params`).

### Terminal-path completeness

Trigger coverage extends to every production path, keeping the invariant
that a terminal queue-entry transition and its `merge_queue.processed`
event cannot diverge (`fail_queue_entry`) and that every task-status
persistence records `task.status_changed` (`record_task_status_changed`):
terminal queue outcomes (quality-gate failure, MR/repo not found,
non-mergeable MR during candidate selection and atomic-group member
checks, dep-health, spec/breaking-change/spec-link/constraint/
attestation-ABAC blocks, git errors, circuit-breaker and operator-cancel
`skipped`), internal task transitions (MCP `update_task`, spawn/completion
auto-advance, admin force-kill, stale-abort, spec-rejection and
repo-archive cancellation), and agent kill paths (fail, admin kill, stale
abort, spec-rejection stop, repo-archive stop, compose teardown). Each
extension has a regression test that fails on the pre-fix code. (The
stale-agent Pause stop stays unemitted by design: pause is a configured
graceful stop, not a failure or kill.)

### Merge and gate hygiene

Reconciled with the evolving base five times (4e895f766/bf39f38a2,
fefde2946, b5a872087) — conflicts resolved as unions, never dropping a
fix or re-adding an exemption. Base's fail-closed
`resolve_orchestrator_persona` and launch-outcome reporting adopted over
this branch's advisory persona warning (the stronger fix of the same
inert-enforcement finding); this branch's `agent.spawned` emission and
regression tests kept alongside base's truthful-launch-outcome test.
Fixed for real, never via exemptions: orchestrator spawn tenant
containment, attestation-ABAC merge enforcement, char-boundary-safe SHA
truncation, spec tenant fanout resolution, GateFailure scope resolution,
fail-closed `resolve_ref` sites, the relative-path literal in spawn.rs,
and the dead traces.rs exemption entry. The fabricated-scope exemption
list intersected across lineages down to one entry
(speculative_merge.rs). The analytics migration is renumbered to the
next unused sequence — `2026-10-08-000059_analytics_scope_columns`.
A wall-clock race in `rate_limit_exhaustion_returns_429` (predates this
branch) was fixed deterministically by injecting the clock into
`RateLimiter`. web/dist is byte-identical to base (this branch never
modified it).

### Review-repair round: workspace-orchestrator spawn containment (commit 3bcd6f525)

Review finding on merge 72da380b3: adopting base's
`is_system_principal()` tenant bypass in `spawn_workspace_orchestrator`
broke this branch's regression test — the global token's cross-tenant
spawn skipped containment, reached the (fail-closed, base-adopted)
persona resolver, and returned 404 instead of the asserted 403, so the
registry-exempt route's only authorization surface no longer held for
every caller.

Fix: the workspace-tier route enforces tenant containment for EVERY
caller, no system-principal bypass — no bootstrap flow spawns a
workspace orchestrator (platform-model.md §8 step 8 spawns the repo
orchestrator only), so no operational path needs the global token to
cross tenants there. The repo-tier route KEEPS its bypass: the
documented dev-mode bootstrap (`gyre bootstrap --dev`, docs/cli.md)
authenticates the step-8 repo-orchestrator spawn with the static global
token against the freshly created "dev" tenant. Both routes' doc
comments record the asymmetry and its reason so a future merge does not
silently re-unify it.

The regression test now seeds the built-in personas in both the foreign
and the caller's tenant: the own-tenant positive control needs an
approved persona in scope (the merged base resolves the spawn persona
fail-closed — the old test environment predates that), and seeding the
foreign tenant proves the 403 comes from containment, not from a missing
persona. Assertion strength unchanged (403 + no agent row for the
foreign workspace; 201 + agent row for the caller's own tenant).

### Test evidence (this round: commit 3bcd6f525 on merge 72da380b3)

- Reproduced the finding first: `cargo test -p gyre-server --test
  api_integration orchestrator_spawn_denied_across_tenants` failed with
  `404 != 403` on the pre-fix tree (evidence:
  /tmp/stage/review-evidence/).
- Post-fix: `--test api_integration` 67 passed / 0 failed (was 66/1);
  `--lib` 1658 passed / 0 failed; `cargo fmt --check` clean; clippy
  clean on the changed files (pre-existing warnings elsewhere only).
- Offline gates exit 0: arch, ABAC registry + exempt handlers,
  forged-scope, fabricated-scope, scope-literal, inert-enforcement,
  migration versions, task-commit attribution (with the new commit
  listed in the frontmatter).
- Full workspace suite, all-target clippy, and GitHub CI remain owned by
  the deterministic verification stage.
