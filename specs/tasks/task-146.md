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
commits: ["8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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

### Analytics event schema and auto-emitted events (commit 38c2c5e)

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

### Terminal-path completeness (review repair rounds b818d20d → 8f0ee1903)

Successive review rounds extended trigger coverage to every production
path, keeping the invariant that a terminal queue-entry transition and its
`merge_queue.processed` event cannot diverge (`fail_queue_entry`), and that
every task-status persistence records `task.status_changed`
(`record_task_status_changed`): terminal queue outcomes (quality-gate
failure, MR/repo not found, dep-health, spec/breaking-change/spec-link/
constraint/attestation-ABAC blocks, git errors, circuit-breaker
`skipped`), internal task transitions (MCP `update_task`, spawn/completion
auto-advance, admin force-kill, stale-abort, spec-rejection and
repo-archive cancellation), and agent kill paths (fail, admin kill,
stale abort, spec-rejection stop, repo-archive stop). The final repair
round (8f0ee1903, on merge bf39f38a2 of base 4e895f766) closed the last
three gaps found by review:

- **`merge_queue.processed` on the two remaining `Failed` transitions** —
  the candidate-selection non-mergeable-MR check (4b'') and the atomic-
  group R4 member non-mergeable check now route through
  `fail_queue_entry(..., "mr_not_mergeable", ...)`; previously both set
  `Failed` with no event while adjacent sites in the same code paths were
  wired.
- **`merge_queue.processed` on operator cancellation** — `cancel_entry`
  (DELETE /api/v1/merge-queue/:id) emits outcome `skipped` via
  `emit_queue_processed_terminal`, same as the circuit-breaker Cancelled
  path.
- **`agent.failed` on the compose teardown kill path** — teardown emits
  `agent_id`/`task_id`/`reason` ("compose session torn down")/
  `duration_secs` for every session agent it marks Dead. (The stale-agent
  Pause stop stays unemitted by design: pause is a configured graceful
  stop, not a failure or kill.)

Each fix has a regression test that fails on the old code:
`non_mergeable_mr_during_selection_records_queue_processed_failed`,
`atomic_group_non_mergeable_member_records_queue_processed_failed`,
`cancel_records_queue_processed_skipped`, and assertions added to
`compose_teardown_stops_all_agents` (probe run with emissions reverted:
4 failures; with fixes: 0).

### Merge and gate hygiene across rounds

- Reconciled with main/base five times (d81b0fe4, a488d9df; 4118553f1,
  28fe73a7f; 4e895f766, bf39f38a2) — conflicts resolved as unions, never
  dropping a fix or re-adding an exemption. The bf39f38a2 merge unioned
  admin.rs (analytics `agent.failed` + task-103 token revocation/lifecycle
  audit) and stale_agents.rs tests, and intersected the fabricated-scope
  exemption fixes: auth.rs fixed by task-103, gate_executor.rs fixed by
  this branch → entries 3 → 1, FROZEN_EXEMPTION_COUNT lowered 3 → 1.
- Migration collisions with base lineages (Diesel runs one migration per
  version) were renumbered to the next unused sequence — currently
  `2026-10-08-000059_analytics_scope_columns`.
- Fixed for real, never via exemptions: orchestrator spawn tenant
  containment, attestation-ABAC merge enforcement, `warn_unapproved_persona`
  rename, char-boundary-safe SHA truncation, spec tenant fanout resolution,
  GateFailure scope resolution, fail-closed `resolve_ref` sites, the
  relative-path literal in spawn.rs, and the dead traces.rs exemption
  entry. `rust_extractor.rs` `never_loop` rewritten for the CI clippy gate.
- web/dist is byte-identical to base (this branch never modified it).

### Test evidence (this round: merge bf39f38a2 + commit 8f0ee1903)

- `cargo test -p gyre-server --lib`: 1517 passed, 0 failed;
  `cargo test -p gyre-server --test api_integration`: 67 passed, 0 failed;
  `cargo test -p gyre-adapters --lib`: 384 passed, 0 failed; `cargo fmt
  --check` clean; `cargo clippy -p gyre-server --lib` exit 0 (warnings
  only, pre-existing).
- All branch-invariant `scripts/check-*.sh` gates pass (migration
  versions/portability, fabricated-scope, scope-literal, relative-path,
  inert-enforcement, abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, arch, task-commit-attribution, and the rest).
- Checks that fail identically on base 4e895f766 (verified in a detached
  worktree at the base commit: 22 mawk-dialect awk errors and the
  pre-existing exit-1 violation sets) are documented in
  /tmp/stage/review-evidence/test-evidence.md — not introduced by this
  branch.
