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
commits: ["e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06", "d1f06da1a8961d2c1d71000d635b1578b7d9b070"]
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

## Shipped

### Analytics event schema and auto-emitted events

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields
  (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`,
  `repo_id`, `properties`, `timestamp`); `with_scope` populates the
  user/session/workspace/repo scope fields. A serde round-trip test
  (`analytics_event_matches_spec_schema`) pins the shape against the spec
  schema.
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
  10_000), `group_by` — implemented on SQLite, Postgres, and mem adapters.
  The sqlite adapter test `analytics_query_filtered_all_params` seeds
  cross-cutting data and asserts each scope filter individually.

### Review-repair: caller-bound identity and scope validation (round 1)

- `record_event` (POST /api/v1/analytics/events) no longer persists
  caller-supplied identity/scope fields verbatim: `agent_id`/`user_id`
  are derived from the verified caller, and `workspace_id`/`repo_id` are
  validated against the caller's tenant via the shared audit.rs
  `validate_workspace_scope`/`validate_repo_scope` helpers — cross-tenant
  attribution is Forbidden, unknown ids NotFound. Regression tests:
  `record_event_binds_identity_to_caller_not_request_body`,
  `record_event_workspace_scope_validated_against_caller_tenant`,
  `record_event_repo_scope_validated_against_caller_tenant`.

### Review-repair: silent terminal paths + search scope (round dfa1266c)

- Ralph-loop gate-failure path records its transitions: `mr.closed`
  (reason `gate_failure`) when the failed MR is closed,
  `task.status_changed` on the Review→InProgress re-open, and
  `agent.failed` + `task.status_changed` at both terminal states — the
  iteration ceiling and the re-spawn retry ladder's exhaustion.
- Every remaining agent-death path emits `agent.failed` via a shared
  `record_agent_failed` helper (api/analytics.rs): worker failed-launch
  teardown (`teardown_failed_spawn`), the orchestrator process-death
  monitor, and the public PUT /api/v1/agents/{id}/status terminal
  statuses (Failed/Dead/Stopped).
- `search_handler` validates the caller-supplied `workspace_id` Query
  param with the same audit.rs `validate_workspace_scope` used by the
  analytics record handler (cross-tenant → Forbidden, unknown →
  NotFound), and the `search.query` event's agent/user identity is bound
  to the verified caller. Regression test:
  `search_workspace_scope_validated_against_caller_tenant` pins a
  forged cross-tenant attribution never appearing in the victim
  workspace's filtered query results.

### Rebase repair: merge with base f98b179 (round 65e4d25e, commit d1f06da1a)

- Resolved the rebase onto base `f98b17925` (task-194 + search access
  scoping). `search.rs`: kept BOTH behaviors — the base's
  `derive_search_scope` caller-scope narrowing (results/facets/suggest
  intersect the caller's accessible workspaces/repos; a same-tenant
  inaccessible workspace yields zero results, never a leak) AND this
  task's tenant-level `workspace_id` validation (cross-tenant → 403,
  unknown → 404, because the param attributes the `search.query`
  analytics event). The two compose: validation gates the persisted
  analytics attribution; scope narrowing gates the returned documents.
  Also removed a merge-artifact duplicate `auth: AuthenticatedAgent`
  parameter (both lineages added `auth` at different positions) that
  would not compile.
- `docs/api-reference.md`: merged the /search, /search/suggest,
  /search/facets rows to describe both the access scoping and the
  workspace_id tenant validation.
- Post-merge verification: all 12 event emitters present; all prior
  regression tests intact; `cargo check -p gyre-server` and
  `--tests` finish clean; `check-arch.sh`, `check-forged-scope-fields.sh`,
  `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-abac-route-registry.sh` pass on the merged tree.

### Verification repair: duplicate migration version (round d88b80e7)

- Renumbered `analytics_scope_columns` to the next unused sequence
  `2026-10-08-000062_analytics_scope_columns` (content unchanged) to
  resolve the duplicate Diesel version against task-163's
  `dep_breaking_policies`; `scripts/check-migration-versions.sh` and
  `scripts/check-migration-sql-portability.sh` pass.

### Test evidence

- In-sandbox (this round, head d1f06da1a): `cargo check -p gyre-server`
  and `cargo check -p gyre-server --tests` finished with 0 errors
  (build wrapper used; 8m37s / 3m44s). Mechanical gates pass on the
  merged tree: `check-arch.sh`, `check-forged-scope-fields.sh`,
  `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-abac-route-registry.sh`.
- Server test binaries exceed this sandbox's 4 GiB build budget — the
  independent verifier must run:
  `cargo test -p gyre-server --lib api::analytics`,
  `cargo test -p gyre-server --lib api::search`,
  `cargo test -p gyre-server --lib ralph_loop`,
  `cargo test -p gyre-server --lib api::agents`,
  `cargo test -p gyre-server --lib signal_chain`,
  `cargo test -p gyre-server --lib api::orchestrator`,
  `cargo test -p gyre-server --test search_wiring`,
  plus the full `cargo test --all` workspace suite and
  `scripts/check-*.sh` gates owned by verification. The merged
  `search_wiring.rs` (base's access-scoping tests + this task's scope
  validation) is the key cross-lineage integration point to exercise.
