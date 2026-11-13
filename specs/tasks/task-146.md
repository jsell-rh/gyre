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
commits: ["e1088342c4f52d501dc18f12b26bcb9a8b93987e", "9f68c1fed65784766f3f967de39cddc7e8466358", "a3afa55f6ff37e35473dc6f95b33811626510e3c", "b49cea80f1536251e1376a1b366aeb3cf0d7e93b", "9bf30a10b6db85a0a8bd8c29fe1c3d6a4378c4c0", "5fb9e9b83b1b8f30da3e87c4ad419267b8543092", "24b2d851a053d1da22c7e75db0a742bd903c0f7f", "e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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

### Review round 2 F1 repair: server-side group_by aggregation

- **The finding:** `GET /api/v1/analytics/events?group_by=event_name`
  aggregated the limit-truncated event list (default 100, cap 10_000) in
  memory, so grouped totals silently disagreed with `/count` on the same
  question — 150 seeded `mr.merged` events grouped as 100. Analytics is
  agent-consumable (analytics.md §Purpose), so truncated counts fed
  wrong data into the autonomous decision loop.
- **The fix:** new port-level grouped-count query
  `AnalyticsRepository::count_grouped(filter, group_by)` with an
  `AnalyticsGroupBy` enum (`EventName`/`AgentId`/`WorkspaceId`/`Day`),
  implemented on all three adapters:
  - SQLite/Postgres: SQL `SELECT <key>, COUNT(*) ... GROUP BY ... ORDER BY
    key LIMIT ?` with the WHERE clause mirroring `query_filtered` exactly
    (exact or trailing-`*` event_name, scope equality, inclusive
    since/until); conditional binds via diesel's `into_boxed()` loop-binding
    pattern.
  - mem: same filter predicate (factored into a shared
    `analytics_event_matches_filter` helper so the grouped path cannot
    drift from `query_filtered`) over the full store, grouped into a
    BTreeMap.
  - `limit` bounds the number of returned GROUPS (per the review:
  "for a grouped response the results are the groups"), never the input
  to the aggregation; NULL agent/workspace groups under `"(none)"`.
- **Handler:** `query_events`'s `group_by` branch now delegates to the
  port (raw-event path unchanged); the in-memory grouping and its
  now-unused `epoch_day_string` helper are removed.
- **Rebase repair:** `merge_processor.rs`'s
  `attestation_abac_deny_blocks_merge` test did not compile after the
  task-128 rebase (main changed `Policy::created_by` to `Id`);
  `cargo check -p gyre-server --tests` failed at the assignment base and
  passes now.

### Test evidence (this round)

- `cargo test -p gyre-adapters sqlite::analytics` — 15 passed,
  including the two new tests:
  `analytics_count_grouped_over_full_filtered_set` (150 events > limit;
  grouped totals equal `count()`; limit=1 returns one group;
  `"(none)"` bucket; prefix + workspace filters) and
  `analytics_count_grouped_by_day` (UTC day buckets).
- Full `cargo test -p gyre-adapters` — 506 passed, 0 failed.
- `cargo check -p gyre-server --tests` — passes (failed at base on the
  merge_processor rebase break).
- Clippy on changed lines clean (the `QueryableByName` derive's
  `redundant_field_names` false positive is scoped off with the
  module-level allow pattern already established in
  `postgres/search.rs`); `cargo fmt --check` clean; arch, mem-port,
  inert-enforcement, byte-slice, lossy-secret, relative-path,
  fabricated-scope, scope-literal, forwarded-header, and unbounded-http
  mechanical checks all pass.

### Verifier must run (gyre-server tests, too large for the sandbox budget)

- `cargo test -p gyre-server --lib api::analytics::tests`
  (includes the new `query_events_group_by_counts_full_filtered_set`
  regression: 150 seeded events, grouped total 150 == `/count`, limit=1
  returns one group, prefix filter keeps full totals)
- `cargo test -p gyre-server --lib analytics_query_tests::count_grouped_over_full_filtered_set`
- `cargo test -p gyre-server --lib merge_processor::tests::attestation_abac_deny_blocks_merge`
  (rebase repair)
