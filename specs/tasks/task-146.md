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
commits: ["02e3945b63c2f28b8e65248d6331b3a3ca7cfce0", "fe61a49ffea2e7e0fe6341ebf261a6a9579ad79a", "08123b833c31d5c56fb62fa332caa58bb6a69257", "f17756774160de561cc1e0a9c1576a43e18b7505", "e1088342c4f52d501dc18f12b26bcb9a8b93987e", "9f68c1fed65784766f3f967de39cddc7e8466358", "a3afa55f6ff37e35473dc6f95b33811626510e3c", "b49cea80f1536251e1376a1b366aeb3cf0d7e93b", "9bf30a10b6db85a0a8bd8c29fe1c3d6a4378c4c0", "5fb9e9b83b1b8f30da3e87c4ad419267b8543092", "24b2d851a053d1da22c7e75db0a742bd903c0f7f", "e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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

The task-146 analytics surface: the event schema with all 9 spec fields,
the query API with the full spec filter set plus server-side `group_by`
aggregation, and all 12 auto-emitted events recorded at their real
trigger points with the spec-required properties.

### This round (review repair 02e3945b)

Review round 920bc33a found four gaps; all are closed in production code:

- **Review→Done terminal transition** — `complete_task_on_merge`
  (ralph_loop.rs) now calls `record_task_status_changed` after persisting
  Done, and the signal-chain scheduler instruments every remaining
  persisted task status transition: orchestrator claim and failure
  release, worker claim, `release_claim`, `final_spawn_failure`, and the
  three delegation/coordination completion paths.
- **Post-merge gate analytics** — `run_post_merge_gates` /
  `run_post_merge_gate_commands` (gate_executor.rs) record
  `gate.passed`/`gate.failed` for every evaluated post-merge command
  gate with gate_id/gate_type/mr_id/output_snippet/duration_secs. The MR
  is threaded from the single-entry merge and revert call sites;
  atomic-group validation and the recovery HEAD check record a null
  mr_id (no single MR is attributable) rather than a fabricated one.
- **Literal prefix filter semantics** — the trailing-`*` event_name
  filter is literal and case-sensitive on all three adapters: SQLite
  uses `substr(event_name, 1, n) = ?` under BINARY collation (LIKE was
  ASCII-case-insensitive and wildcard-honoring), Postgres escapes
  `%`/`_`/backslash in its case-sensitive LIKE, mem already used
  `starts_with`. The port contract doc pins the semantics; the unused
  unescaped `event_name_like` helper is removed.
- **UTC day grouping on Postgres** — `count_grouped` and
  `aggregate_by_day` render `to_char(... AT TIME ZONE 'UTC', ...)`, so
  day buckets match the SQLite/mem adapters and the port contract on
  non-UTC deployments.

### Test evidence

Run in this sandbox (focused probes; the build wrapper defers gyre-server
test execution to the independent verifier):

- `cargo test -p gyre-adapters` — 516 passed, 0 failed (includes the new
  `analytics_event_name_prefix_is_literal_case_sensitive` regression on
  both query_filtered and count_grouped, and the pre-existing
  `analytics_query_filtered_all_params` / grouped-counts suites).
- `cargo check -p gyre-server --lib --tests` — passes (all server test
  edits compile).
- `cargo clippy -p gyre-server --lib` / `-p gyre-adapters` — no warning
  on any line this branch changed vs the base.
- `cargo fmt -- --check` — clean.
- `scripts/check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-byte-slice-truncation.sh`, `check-fabricated-scope-defaults.sh`,
  `check-scope-literal-defaults.sh`, `check-lossy-secret-conversion.sh`,
  `check-inert-enforcement.sh`, `check-relative-path-defaults.sh`,
  `check-fail-open-ref-resolution.sh`, `check-forwarded-header-trust.sh`,
  `check-unbounded-external-http.sh`,
  `check-migration-sql-portability.sh` — all pass.

Server tests the verifier must execute (edited this round):

- `cargo test -p gyre-server ralph_loop::tests::merge_completes_task` —
  now also asserts the Review→Done `task.status_changed` event.
- `cargo test -p gyre-server signal_chain::tests::scheduler_decomposes_delegation_into_ordered_subtasks`
  — asserts claim/completion/worker-claim events.
- `cargo test -p gyre-server signal_chain::tests::worker_spawn_failure_retries_then_blocks`
  — asserts the final-failure Block event.
- `cargo test -p gyre-server merge_processor` — includes the extended
  post-merge gate pass/fail analytics assertions.
- `cargo test -p gyre-server mem::tests::query_filtered_matches_sqlite_filter_semantics`
  — extended with case/wildcard-rejection assertions.
- Plus the pre-existing analytics suites (`api::analytics`,
  `gate_executor`, `mcp`) unchanged by this round.

Postgres adapter changes are compile-checked only (no PG server in this
sandbox); the SQL is the same `AT TIME ZONE 'UTC'` / escaped-LIKE
constructs already exercised by the SQLite and mem adapters' tests.
