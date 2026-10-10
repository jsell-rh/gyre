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
commits: ["38c2c5e777fb71633ba6106f6cb8ff037c58e4ff"]
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

- Recovery repair (attempt 9873223d, then re-merged onto newer main as
  3802b8d9): an earlier interrupted attempt had rewritten this file's
  `commits:` list away from the assignment's nine SHAs. The nine-SHA list
  above is the assignment's, exactly as issued; all nine resolve in local
  and remote refs. The implementation also landed on this branch as
  checkpoint `38c2c5e7` (via merge `1678b05b`), whose only product-surface
  files are the analytics schema/migration/adapter changes listed there.
- Contract repair round 3802b8d9: no analytics product code changed
  (`git diff 73a31e0b HEAD -- crates/gyre-server/src/api/mod.rs` is empty —
  analytics routes byte-identical to base, no new endpoints, per the task's
  own instruction); re-verified the merged tree (base `73a31e0b` absorbed
  task-200/213/219) and recorded fresh evidence.
- Contract repair round e08e8997: the post-repair bookkeeping commit
  `560e7d91` had again overwritten the nine-SHA `commits:` list with the
  checkpoint SHA, unchecked the acceptance boxes, and dropped the Shipped
  section. Restored the contract file. No product code changed that round
  either: merge `c49926a1` (base `653a696f`, absorbing main's task-222)
  touched only spec/task files.
- Contract repair round 157407eb (this round): HEAD's committed task file
  had again been reduced to the single checkpoint SHA by pipeline
  bookkeeping (`6ae9eefb`), and the merge `86a66cf0` (base `a11ba8d3`,
  absorbing main's task-068) changed `crates/` — so last round's
  "analytics code byte-identical" claim no longer held globally. This
  round re-verified analytics behavior directly at HEAD: the task-068
  delta (`mcp.rs` +499, `view_query_resolver.rs` +338, `explorer_ws.rs`,
  `graph_integration.rs`) contains zero analytics emit-site changes
  (0 hits for `record_event`/`"task.status_changed"`/`"agent.completed"`
  in `git diff 3c14d294 HEAD -- crates/`), and every focused suite was
  re-run at HEAD (see Shipped). Also repaired main-side attribution
  drift absorbed by the merge: `a11ba8d3` (task-068's product commit)
  added to task-068's `commits:` frontmatter per the gate script's
  prescribed fix — `origin/main` fails `check-task-commit-attribution.sh`
  identically without it; no exemption file touched.

## Shipped

- Event schema: `AnalyticsEvent` (`gyre-domain/src/analytics.rs`) carries
  all 9 spec fields (id, event_name, agent_id, user_id, session_id,
  workspace_id, repo_id, properties, timestamp); `with_scope` populates the
  four scope fields. `analytics_event_matches_spec_schema` pins the shape
  via serde round-trip. Id-typed fields are stored as text (`Option<String>`)
  matching the storage layer's TEXT columns.
- All 12 auto-emitted events record at real trigger points with the
  spec-required properties. Emit sites re-verified at HEAD `86a66cf0`
  (line numbers drifted +13 in mcp.rs and +17 in merge_processor.rs since
  the task-068 merge; all other sites unchanged):
  task.status_changed (api/tasks.rs:347 REST, mcp.rs:1064 MCP),
  mr.merged (api/merge_requests.rs:666 HTTP transition,
  merge_processor.rs:1864 `emit_mr_merged` for both queue paths incl.
  atomic groups), mr.closed (api/merge_requests.rs:666 HTTP transition,
  api/repos.rs:320 repo-archive, api/specs.rs:938 spec-reject),
  agent.spawned (api/spawn.rs:1086 direct, api/orchestrator.rs:143
  orchestrator tiers), agent.completed (api/spawn.rs:1323 HTTP,
  mcp.rs:1454 MCP), agent.failed (api/spawn.rs:1477 fail,
  api/spawn.rs:1563 stop, api/admin.rs:339 admin-kill, stale_agents.rs:43
  heartbeat-abort), merge_queue.processed (merge_processor.rs:759 group,
  :1536 single, :1823 `emit_queue_processed_failed`), gate.failed /
  gate.passed (gate_executor.rs:104/:114), spec.approved
  (api/specs.rs:703), budget.warning (api/budget.rs:270
  `emit_budget_warning`, called from the spawn-path budget check and usage
  recording), search.query (api/search.rs:84).
- Query API: routes byte-identical to base in `api/mod.rs` (verified —
  no diff vs `a11ba8d3`). `POST/GET /api/v1/analytics/events` supports
  `event_name` (incl. trailing-`*` prefix wildcard on SQLite and
  Postgres), `agent_id`, `user_id`, `workspace_id`, `repo_id`,
  `since`/`until` (ISO8601 or unix secs), `limit` (default 100, max
  10_000), `group_by` (event_name/agent_id/workspace_id/day; unsupported
  fields rejected with 400). `GET /api/v1/analytics/count` and `/daily`
  cover count and daily aggregation.
- Test evidence (all re-run this round at HEAD `86a66cf0`, all exit 0):
  gyre-domain `--lib analytics` 3/3 (incl.
  `analytics_event_matches_spec_schema`); gyre-adapters `--lib
  sqlite::analytics` 13/13 (incl. `analytics_query_filtered_all_params`:
  prefix wildcard, conjunctive scope filters, inclusive since/until
  bounds, limit); gyre-server `--lib api::analytics` 20/20; per-event
  batch 20/20 (task.status_changed ×2 REST+MCP, agent.spawned ×2,
  agent.completed ×2, agent.failed ×3 fail/admin-kill/stale-abort,
  mr.closed ×3 transition/repo-archive/spec-reject, mr.merged ×2,
  merge_queue.processed via queue-merge + atomic-group tests, gate ×2,
  spec.approved, budget.warning positive, search.query positive);
  negative paths: `budget_warning_not_emitted_below_threshold` and
  `empty_query_emits_no_analytics_event` both pass — every spec-required
  property asserted.
- No verifier weakened: `scripts/check-task-commit-attribution.sh` exits
  0 on the final tree (re-run after the task-068 frontmatter repair);
  exemption files unchanged (frozen count 3).
- Sandbox cannot bind a TCP listener (errno 95, /tmp/stage/capabilities.json),
  so no live HTTP probe was run here; exact-head GitHub checks remain the
  transport verification for `POST/GET /api/v1/analytics/events` (all
  filter params), `/count`, `/daily`.
