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
commits: ["5f0602675167018a09ef08ff6adbfcafc7b612cd", "3eebbbb5ac640428868a8f5eb4ae229675c86891", "1f8277301936405de86d3ff5269304a61ab74a44", "dd84d9d00a5b69111ee5a8c131db0c5d1ead08bc", "ee479add261ad42c61d4044eecbbdca8ed6263c9", "a5bc917785f7c2e248e84e4d62117439fc08221e", "fe6a6642c7bbd8331bab9be3ce61164a33058075"]
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

## Shipped

All five assigned coverage sections are implemented and verified at head `3d9ab812` (all 7 attributed commits are ancestors).

- **Event Schema** — `gyre-domain::AnalyticsEvent` carries every spec field (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`, `repo_id`, `properties`, `timestamp`); scope fields set via `with_scope()` at every emit site. SQLite/PG migration `2026-10-08-000056_analytics_scope_columns` adds the columns + indexes; `AnalyticsQueryFilter::query_filtered` enforces them in both adapters (mem adapter matches SQLite semantics — tested).
- **Auto-Emitted Events** — all 12 events recorded at real trigger points: `task.status_changed` (HTTP + MCP paths), `mr.merged` (queue + HTTP paths, real `gate_count` from gate_results, `queue_wait_secs` from queue entry), `mr.closed` (HTTP close, repo archive, spec-reject), `agent.spawned` (worker + orchestrator with persona), `agent.completed` (completion + MCP), `agent.failed` (fail handler, admin kill, stale-agent abort), `merge_queue.processed` (merged/failed/skipped outcomes), `gate.failed`/`gate.passed` (gate executor, char-boundary-safe `output_snippet` truncation), `spec.approved`, `budget.warning` (80% threshold, tokens/cost/agents metrics), `search.query` (real measured `duration_ms`, deduped `entity_types`). Every event includes all spec-required properties.
- **Query API / Parameters** — `GET /api/v1/analytics/events` supports `event_name` (exact + trailing-`*` prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs), `limit` (default 100, cap 10000), `group_by` (`event_name`/`agent_id`/`workspace_id`/`day`, invalid field → 400). No new endpoints added. `POST /events`, `GET /count`, `GET /daily` (`?days=30` form) unchanged.

Test evidence (focused probes, exit 0): `gyre-domain --lib analytics` 3 passed (incl. `analytics_event_matches_spec_schema`); `gyre-server --lib -- analytics` 42 passed — one trigger test per event asserting recorded properties; budget/orchestrator/repos/stale-agent/admin event tests 52 passed; `gyre-adapters --lib -- analytics` 13 passed (SQLite record/query/filter/count/daily/retention). Details: `/tmp/stage/review-evidence/task-146-verification.md`.

Note: this sandbox cannot accept TCP listeners (errno 95, `capabilities.json`), so verification is via the in-process router tests above; live HTTP smoke belongs to host/CI verification.

Closeout (verification session, head `f3ea040d`): the branch's code insertions had drifted four line-number-pinned exemption files off their pre-existing constructs, breaking `check-inert-enforcement`, `check-abac-exempt-handlers`, and `check-fabricated-scope-defaults` (pass at base, fail at head). Re-pinned the same entries to current lines — entry counts and frozen baselines unchanged, no new exemptions (commits `79fa2dd5`, `f3ea040d`); all four affected checks exit 0. Also attributed pre-existing task-210 commit `a781ede2` in its frontmatter (commit `6ae8207f`) so `check-task-commit-attribution` exits 0. Remaining repo-check failures are byte-identical at base `8c2d1775` and head (verified via worktree diff) — pre-existing, owned elsewhere. Full audit: `/tmp/stage/review-evidence/task-146-verification.md`.

## Agent Instructions

- Read `crates/gyre-server/src/api/analytics.rs` for existing analytics implementation
- Read `crates/gyre-server/src/api/mod.rs` lines 430-438 for existing analytics routes
- Grep for `analytics.record` to find all existing auto-emit call sites
- Read `crates/gyre-server/src/api/spawn.rs` for agent lifecycle events
- Read `crates/gyre-server/src/merge_processor.rs` for merge/gate events
- Read `crates/gyre-server/src/api/specs.rs` for spec approval events
- Read `crates/gyre-server/src/api/search.rs` for search endpoint
- Do NOT create new endpoints — the query API routes already exist. Only add missing auto-emitted events and verify completeness.
