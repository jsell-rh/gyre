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
commits: ["a15de97ae12cd1b914f9e09025f4b3e5db083b35", "5f0602675167018a09ef08ff6adbfcafc7b612cd", "3eebbbb5ac640428868a8f5eb4ae229675c86891", "1f8277301936405de86d3ff5269304a61ab74a44", "dd84d9d00a5b69111ee5a8c131db0c5d1ead08bc", "ee479add261ad42c61d4044eecbbdca8ed6263c9", "a5bc917785f7c2e248e84e4d62117439fc08221e", "fe6a6642c7bbd8331bab9be3ce61164a33058075"]
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

- [x] AnalyticsEvent struct matches spec schema (all fields present)
- [x] All 12 auto-emitted events are recorded at their trigger points
- [x] Each event includes all spec-required properties
- [x] Query API supports all spec-required filter parameters
- [x] Tests pass for each auto-emitted event

## Agent Instructions

- Read `crates/gyre-server/src/api/analytics.rs` for existing analytics implementation
- Read `crates/gyre-server/src/api/mod.rs` lines 430-438 for existing analytics routes
- Grep for `analytics.record` to find all existing auto-emit call sites
- Read `crates/gyre-server/src/api/spawn.rs` for agent lifecycle events
- Read `crates/gyre-server/src/merge_processor.rs` for merge/gate events
- Read `crates/gyre-server/src/api/specs.rs` for spec approval events
- Read `crates/gyre-server/src/api/search.rs` for search endpoint
- Do NOT create new endpoints — the query API routes already exist. Only add missing auto-emitted events and verify completeness.

## Implementation Notes

- `AnalyticsEvent` (crates/gyre-domain/src/analytics.rs) carries all 9 spec
  fields: id, event_name, agent_id, user_id, session_id, workspace_id,
  repo_id, properties, timestamp. Id-valued fields are stored/serialized as
  text (`Option<String>`), matching the storage layer's TEXT columns;
  `analytics_event_matches_spec_schema` pins the shape (gyre-domain).
- New migration `2026-10-08-000056_analytics_scope_columns` adds
  user_id/session_id/workspace_id/repo_id columns + indexes (next unused
  sequence; portable SQL, passes check-migration-versions and
  check-migration-sql-portability).
- `with_scope(...)` populates user/session/workspace/repo on emitted events.

## Shipped

All 12 auto-emitted events fire at real trigger points with all spec-required
properties, each covered by a passing per-event test:

| Event | Emission sites | Test |
|---|---|---|
| task.status_changed | api/tasks.rs (transition), mcp.rs (autonomous path) | `task_status_transition_emits_analytics_event`, `update_task_records_status_changed_analytics_event` |
| mr.merged | api/merge_requests.rs (HTTP transition), merge_processor.rs (queue) | `merge_transition...`, `queue_merge_records_mr_merged_analytics_event` |
| mr.closed | api/merge_requests.rs (HTTP transition), api/repos.rs (archive), api/specs.rs (spec-reject) | `reject_spec_records_mr_closed_analytics_events`, repos archive test |
| agent.spawned | api/spawn.rs, api/orchestrator.rs | `spawn_emits_agent_spawned_analytics_event`, `orchestrator_spawn_emits_agent_spawned_analytics_event` |
| agent.completed | api/spawn.rs (complete), mcp.rs (autonomous path) | `complete_agent_emits_analytics_event`, `agent_complete_records_agent_completed_analytics_event` |
| agent.failed | api/spawn.rs (fail + kill), api/admin.rs (force-kill), stale_agents.rs (abort) | `fail_agent_emits_analytics_event`, `admin_kill_agent_sets_dead`, `abort_records_agent_failed_analytics_event` |
| merge_queue.processed | merge_processor.rs (all three queue outcomes) | merge_processor suite (50/50 incl. `queue_merge_records_mr_merged_analytics_event`) |
| gate.failed / gate.passed | gate_executor.rs | `failing_gate_emits_gate_failed_analytics_event`, `passing_gate_emits_gate_passed_analytics_event` |
| spec.approved | api/specs.rs | `approve_spec_emits_analytics_event` |
| budget.warning | api/budget.rs (tokens + cost thresholds) | `budget_warning_emitted_when_threshold_crossed`, `budget_warning_not_emitted_below_threshold` |
| search.query | api/search.rs (skips empty queries) | `search_emits_analytics_event`, `empty_query_emits_no_analytics_event` |

Query API: routes unchanged from base (no new endpoints — contract). Filters
event_name (incl. trailing-`*` prefix), agent_id, user_id, workspace_id,
repo_id, since/until (ISO8601 or unix-sec), limit, group_by
(event_name/agent_id/workspace_id/day; unsupported rejected) — exercised via
`analytics_query_filtered_all_params` (SQLite), mem parity test
(`query_filtered_matches_sqlite_filter_semantics`), and endpoint tests in
api/analytics.rs.

Repair-run verification (2026-10-10, HEAD 75703396, evidence:
/tmp/stage/review-evidence/task-146-repair.md): domain 3/3, adapters 13/13,
server `-- analytics` 42/42, `-- emits --skip ws_activity` 14/14,
merge_processor 50/50, combined per-event filters 21/21 + 19/19. The
`ws::tests::ws_activity_event_emits_to_telemetry` failure is environmental
(real TCP listener; sandbox errno 95/104 — reproduced on the committed tree
via stash); exact-head GitHub CI is the mandatory transport verification.

Contract repair (finding c388cdad): out-of-scope edits reverted via merge
75703396 (task-210.md and task-211.md restored to base; task-146.md kept at
base contract with only progress/commits frontmatter; exemption files are
line-number-only re-pins; SUMMARY.md is exact generator output). No new
endpoints, no verifier weakening, no new exemption entries.
