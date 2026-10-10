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
commits: ["7aec532de81863e9de2791434778c292838335a1", "a15de97ae12cd1b914f9e09025f4b3e5db083b35", "5f0602675167018a09ef08ff6adbfcafc7b612cd", "3eebbbb5ac640428868a8f5eb4ae229675c86891", "1f8277301936405de86d3ff5269304a61ab74a44", "dd84d9d00a5b69111ee5a8c131db0c5d1ead08bc", "ee479add261ad42c61d4044eecbbdca8ed6263c9", "a5bc917785f7c2e248e84e4d62117439fc08221e", "fe6a6642c7bbd8331bab9be3ce61164a33058075"]
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

- The nine SHAs in `commits:` are the assignment's, exactly as issued; all
  resolve in local refs (original task-146 implementation lineage, carried
  into this branch via checkpoint `38c2c5e7`).
- Contract repair round `5cff2190`: candidate `54f7ecd1` was pure pipeline
  bookkeeping (one-line diff rewriting the nine-SHA list to the checkpoint
  SHA `38c2c5e7`). Restored the assignment contract. The round's merge
  `430014c7` (base `27bd585c`, absorbing main's task-155) changed only
  `gyre-cli` files and docs — zero files under `crates/gyre-server`, and the
  only analytics-keyword matches in the delta are gyre-cli search-CLI help
  text — so the analytics implementation is byte-identical to the
  last-verified state. All 12 emit sites re-verified by direct read at HEAD
  `430014c7` and every focused suite re-run (see Shipped).
- Also repaired this round's absorbed attribution drift: `27bd585c` is
  task-155's product commit (gyre-cli search CLI), missing from task-155's
  `commits:` frontmatter — the gate fails identically at base itself
  (verified with the task-146 file stashed). Fixed by recording the SHA in
  task-155's frontmatter per the gate's prescribed fix. No exemption file
  touched (frozen count 3).

## Shipped

- Event schema: `AnalyticsEvent` (`crates/gyre-domain/src/analytics.rs:11-21`)
  carries all 9 spec fields (id, event_name, agent_id, user_id, session_id,
  workspace_id, repo_id, properties, timestamp); `with_scope()` (:46-58)
  populates the four scope fields. `analytics_event_matches_spec_schema`
  (:135-168) pins the shape via serde round-trip. Id-typed fields are stored
  as `Option<String>` matching the storage layer's TEXT columns.
- All 12 auto-emitted events record at real trigger points with the
  spec-required properties. Emit sites verified at HEAD `430014c7`:
  task.status_changed (api/tasks.rs:347 REST, mcp.rs:1064 MCP),
  mr.merged (api/merge_requests.rs:666 HTTP transition,
  merge_processor.rs:1847 `emit_mr_merged` for both queue paths incl.
  atomic groups), mr.closed (api/merge_requests.rs:666 HTTP transition,
  api/repos.rs:318 repo-archive, api/specs.rs:938 spec-reject),
  agent.spawned (api/spawn.rs:1086 direct, api/orchestrator.rs:143
  orchestrator tiers), agent.completed (api/spawn.rs:1323 HTTP,
  mcp.rs:1453 MCP), agent.failed (api/spawn.rs:1477 fail, api/spawn.rs:1563
  stop, api/admin.rs:339 admin-kill, stale_agents.rs:43 heartbeat-abort),
  merge_queue.processed (merge_processor.rs:759 group, :1536 single, :1823
  `emit_queue_processed_failed`), gate.failed / gate.passed
  (gate_executor.rs:104/:114), spec.approved (api/specs.rs:703),
  budget.warning (api/budget.rs:270 `emit_budget_warning`, called from the
  spawn-path budget check and usage recording), search.query
  (api/search.rs:84).
- Query API: routes unchanged from base in `api/mod.rs:449-453`. `POST/GET
  /api/v1/analytics/events` supports `event_name` (incl. trailing-`*`
  prefix wildcard on SQLite and Postgres), `agent_id`, `user_id`,
  `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs),
  `limit` (default 100, max 10_000), `group_by`
  (event_name/agent_id/workspace_id/day; unsupported fields rejected with
  400). `GET /api/v1/analytics/count` and `/daily` cover count and daily
  aggregation.
- Test evidence (all run this round at HEAD `430014c7`, all exit 0; evidence
  file /tmp/stage/review-evidence/task-146-round-5cff2190.txt): gyre-domain
  `--lib analytics` 3/3 (incl. `analytics_event_matches_spec_schema`);
  gyre-adapters `--lib sqlite::analytics` 13/13 (incl.
  `analytics_query_filtered_all_params`: prefix wildcard, conjunctive scope
  filters, inclusive since/until bounds, limit); gyre-server `--lib
  api::analytics` 20/20; per-event batch 19/19 + admin-kill 1/1
  (task.status_changed REST+MCP, agent.spawned direct+orchestrator,
  agent.completed HTTP+MCP, agent.failed fail-path+admin-kill+stale-abort,
  mr.closed HTTP transition+spec-reject, mr.merged HTTP+queue with
  gate_count/queue_wait_secs, gate ×2, spec.approved, budget.warning
  positive, search.query positive); gyre-server `--lib merge_processor`
  50/50 (queue-merge and atomic-group suites assert merge_queue.processed /
  mr.merged with wait_secs); negative paths:
  `budget_warning_not_emitted_below_threshold` and
  `empty_query_emits_no_analytics_event` both pass — every spec-required
  property asserted at each site.
- No verifier weakened: `scripts/check-task-commit-attribution.sh` exits 0
  on the final tree; exemption files unchanged (frozen count 3).
- Sandbox cannot bind a TCP listener (errno 95,
  /tmp/stage/capabilities.json), so no live HTTP probe was run here;
  exact-head GitHub checks remain the transport verification for
  `POST/GET /api/v1/analytics/events` (all filter params), `/count`,
  `/daily`.
