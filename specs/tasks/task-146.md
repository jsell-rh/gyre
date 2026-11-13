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
commits: ["a3afa55f6ff37e35473dc6f95b33811626510e3c", "b49cea80f1536251e1376a1b366aeb3cf0d7e93b", "9bf30a10b6db85a0a8bd8c29fe1c3d6a4378c4c0", "5fb9e9b83b1b8f30da3e87c4ad419267b8543092", "24b2d851a053d1da22c7e75db0a742bd903c0f7f", "e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec
  fields (`id`, `event_name`, `agent_id`, `user_id`, `session_id`,
  `workspace_id`, `repo_id`, `properties`, `timestamp`); `with_scope`
  populates the user/session/workspace/repo scope fields. A serde
  round-trip test (`analytics_event_matches_spec_schema`) pins the shape
  against the spec schema.
- All 12 spec-required auto-emitted events record at their real trigger
  points with all spec-required properties, each proven by a per-event
  test: `task.status_changed`, `mr.merged`, `mr.closed`, `agent.spawned`,
  `agent.completed`, `agent.failed`, `merge_queue.processed`,
  `gate.failed`, `gate.passed`, `spec.approved`, `budget.warning`,
  `search.query`.
- Query API: `POST/GET /api/v1/analytics/events`, `GET /count`,
  `GET /daily` with the full spec filter set — `event_name` (exact or
  trailing-`*` prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`,
  `since`, `until` (ISO8601 or unix seconds), `limit` (default 100, cap
  10_000), `group_by` — implemented on SQLite, Postgres, and mem
  adapters.
- Caller-bound identity: `record_event` derives `agent_id`/`user_id`
  from the verified caller and validates `workspace_id`/`repo_id`
  against the caller's tenant; both `search.query` emission paths (the
  HTTP handler and the MCP `gyre_search` tool) bind the event identity
  to the caller the same way.
- Persona/compute-target attribution: `agent.spawned` reports the
  resolved persona slug and compute target at the worker spawn,
  orchestrator spawn, and ralph-loop re-spawn sites.

### Rebase reconciliation with main

- task-119 budget cascade (commit `b57cfc60`): kept main's effective
  workspace-config structure in `check_spawn_budget` with the
  `budget.warning` emissions on all three metric arms; kept both the
  `agent.failed` event and main's budget slot release / compute-minutes
  charge in the admin kill path.
- task-139 signed-approval spec policies (commit `5c5fdb42`): resolved
  `merge_processor.rs` conflicts so both features survive —
  `require_signed_approval` / `require_approved_spec` / stale-spec
  violations now fail the queue entry through `fail_queue_entry`, which
  keeps the terminal status change and the spec-required
  `merge_queue.processed` analytics event tied together, then
  `report_spec_policy_violation` notifies the MR author (GateFailure
  workspace event + inbox notification) and `record_stale_spec_advisory`
  records the warn-only MR comment, exactly as task-139 specced.

### Test evidence (this round)

- `cargo check -p gyre-server` clean at the merge commit (exit 0).
- `cargo test -p gyre-domain analytics`: 3 passed (spec schema pin
  included).
- `cargo test -p gyre-adapters analytics`: 13 passed (query filters,
  count, daily aggregation, limit, since, purge).

### Verifier test plan (gyre-server, run post-model)

- `cargo test -p gyre-server analytics` — query API + scope filters
  (`handle_analytics_query`, `analytics_event(s)`, per-filter sqlite
  test `897b5565` suite).
- Per-event tests: `task_status_transition_emits_analytics_event`,
  `update_task_records_status_changed_analytics_event`,
  `merging_mr_emits_analytics_event_with_gate_count`,
  `closing_mr_emits_analytics_event`,
  `reject_spec_records_mr_closed_analytics_events`,
  `spawn_emits_agent_spawned_analytics_event`,
  `orchestrator_spawn_emits_agent_spawned_analytics_event`,
  `spawn_analytics_persona_reports_bound_persona`,
  `orchestrator_spawn_analytics_reports_resolved_compute_target`,
  `complete_agent_emits_analytics_event`,
  `agent_complete_records_agent_completed_analytics_event`,
  `fail_agent_emits_analytics_event`,
  `abort_records_agent_failed_analytics_event`,
  `failing_gate_emits_gate_failed_analytics_event`,
  `passing_gate_emits_gate_passed_analytics_event`,
  `approve_spec_emits_analytics_event`,
  `budget_warning_emitted_when_threshold_crossed`,
  `budget_warning_not_emitted_below_threshold`,
  `search_emits_analytics_event`,
  `gyre_search_records_search_query_analytics_event`,
  `empty_query_emits_no_analytics_event`,
  `gate_failure_during_selection_records_queue_processed_failed`,
  `cancel_records_queue_processed_skipped` (merge_queue.processed).
- Merge-resolution guards (task-139 × task-146 interaction):
  `spec_policy_block_records_queue_processed_failed`,
  `require_current_spec_blocks_stale_spec_ref`,
  `require_signed_approval_blocks_unsigned_spec_approval`,
  `queue_merge_records_mr_merged_analytics_event`.
- `cargo test -p gyre-domain --lib analytics` — spec schema pin.
