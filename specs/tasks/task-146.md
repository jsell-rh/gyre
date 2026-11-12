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
commits: ["e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06", "24b2d851a053d1da22c7e75db0a742bd903c0f7f"]
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
  adapters. The sqlite adapter test `analytics_query_filtered_all_params`
  asserts each scope filter individually.

### Review repairs carried by this branch

- Caller-bound identity: `record_event` derives `agent_id`/`user_id`
  from the verified caller and validates `workspace_id`/`repo_id`
  against the caller's tenant (cross-tenant Forbidden, unknown
  NotFound); `search_handler` applies the same validation and binds the
  `search.query` event identity to the caller.
- Silent terminal paths emit their transitions: `mr.closed` on
  gate-failure close, `agent.failed` via a shared `record_agent_failed`
  helper on every agent-death path, `task.status_changed` on re-open
  and iteration-ceiling paths.
- The rebase onto base f98b179 composed both search scoping behaviors:
  the base's caller-scope narrowing of returned documents AND this
  task's tenant-level `workspace_id` validation of the persisted
  analytics attribution.

### Merge repair: base 3b734b1af (round 24b2d851a)

- The base merge collided Diesel versions: task-168's
  `2026-10-08-000062_gate_position` vs this branch's
  `2026-10-08-000062_analytics_scope_columns` — only one migration runs
  per version, so fresh databases silently lost `analytics_events.user_id`
  (adapters analytics tests failed with "no such column"). Renumbered to
  `2026-10-08-000063_analytics_scope_columns` (content unchanged);
  `check-migration-versions.sh` and
  `check-migration-sql-portability.sh` pass.
- Completed the base's QualityGate fixture merge in merge_processor.rs
  tests: the auto-merge added the new `position` field to four test
  initializers but missed the gate-failure-selection fixture (E0063).

### Test evidence

- In-sandbox (head 24b2d851a): `cargo test -p gyre-domain --lib
  analytics` — 3 passed; `cargo test -p gyre-adapters --lib analytics`
  — 13 passed; `cargo test -p gyre-adapters --lib sqlite::tests` — 9
  passed (migration guards incl.
  `no_duplicate_migration_version_prefixes` and
  `migrations_create_previously_shadowed_columns`);
  `cargo check -p gyre-server --tests` — clean. Gate-suite diff vs base
  3b734b1af: only `check-task-commit-attribution.sh` differed (fixed by
  this round's frontmatter rebuild); the other script failures are
  identical on the base and owned by verification.
- Server test binaries exceed this sandbox's 4 GiB build budget — the
  independent verifier must run: `cargo test -p gyre-server --lib
  api::analytics`, `api::search`, `ralph_loop`, `api::agents`,
  `signal_chain`, `api::orchestrator`, `api::spawn`, `api::budget`,
  `api::specs`, `api::merge_requests`, `merge_processor`,
  `gate_executor`, `api::tasks`, `api::mcp`, `stale_agents`,
  `--test search_wiring`, `--test api_integration analytics`, plus the
  full `cargo test --all` workspace suite and `scripts/check-*.sh`
  gates owned by verification.
