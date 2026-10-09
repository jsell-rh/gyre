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

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries every spec field:
  id, event_name, agent_id, user_id, session_id, workspace_id, repo_id,
  properties, timestamp. Id-valued fields are stored as text
  (`Option<String>`) — consistent with the pre-existing `agent_id: Option<String>`
  and the storage layer's TEXT columns; the serde round-trip test pins the shape.
- All 12 spec auto-emitted events now emit at their real trigger points
  (see the emit-site table in /tmp/stage/review-evidence/task-146-evidence.md
  at checkpoint time; canonical sites: tasks.rs:345, mcp.rs:1049,
  merge_requests.rs:681, merge_processor.rs:757/1534/1821/1862, repos.rs:318,
  specs.rs:701/936, spawn.rs:1084/1321/1475/1561, orchestrator.rs:141,
  admin.rs:337, stale_agents.rs:41, gate_executor.rs:130, budget.rs:268,
  search.rs:82). Multiple trigger paths are covered per event where the
  spec's "fails or is killed" wording names several paths (agent.failed:
  fail_agent, admin force-kill, stale-abort; mr.closed: HTTP transition,
  repo deletion, spec-reject).
- Query API: `QueryEventsParams` supports event_name, agent_id, user_id,
  workspace_id, repo_id, since, until, limit (default 100, max 10_000),
  group_by (event_name/agent_id/workspace_id/day). `event_name` supports
  the spec's trailing-`*` prefix form (`mr.*`) via LIKE in BOTH the SQLite
  and Postgres adapters. since/until accept ISO8601 or unix seconds.
- Contract repairs kept intact from earlier rounds: the pre-existing
  `mr.created` event (spawn.rs, emitted on agent completion MR creation)
  and the spec_index `&e.current_sha[..8]` hex-sha slice + its exemption
  entry were restored to base form; exemption-file diffs vs base are
  line-number re-pins only (no new entries, no check weakened).

## Shipped

- Event schema: `AnalyticsEvent` has all 9 spec fields; unit test
  `analytics_event_matches_spec_schema` asserts each field and a serde
  round-trip (gyre-domain, 3/3 pass).
- Auto-emitted events: all 12 recorded at real trigger points with all
  spec-required properties. Per-event tests pass: task.status_changed
  (old_status/new_status/task_id/assigned_to), mr.merged (mr_id/repo_id/
  gate_count/queue_wait_secs — both the HTTP transition and queue merge
  paths), mr.closed (mr_id/repo_id/reason), agent.spawned (agent_id/
  task_id/compute_target/persona — direct and orchestrator paths),
  agent.completed (agent_id/task_id/duration_secs), agent.failed
  (agent_id/task_id/reason — fail, admin force-kill, stale-abort paths),
  merge_queue.processed (mr_id/outcome/wait_secs), gate.failed
  (gate_id/gate_type/mr_id/output_snippet), gate.passed (gate_id/
  gate_type/mr_id/duration_secs), spec.approved (spec_path/approver_type/
  approval_mode), budget.warning (workspace_id/metric/threshold_pct —
  plus a not-emitted-below-threshold negative test), search.query
  (query_length/entity_types/result_count/duration_ms — plus an
  empty-query negative test).
- Query API: all spec filter parameters work, including trailing-`*`
  event-name prefix matching on both storage adapters
  (`analytics_query_filtered_all_params`, 13/13 adapter tests pass).
- Evidence: domain 3/3, adapters 13/13, server per-event suites
  (9 + 6 + merge_processor 50/50 incl. queue-merge analytics, stale-abort,
  admin kill) all green; full mechanical check suite A/B vs assignment
  base shows identical failure sets (36 pre-existing both sides, zero
  regressions — prior round's "expected 35" was a miscount of the same
  baseline). Sandbox cannot bind a TCP listener (errno 95), so no live
  HTTP probe; exact-head GitHub CI remains the transport verification.
