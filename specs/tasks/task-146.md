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
commits: ["ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06", "f301cff2c388a93a12566692550cd4420f69e9db"]
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

### Review-repair: caller-bound identity and scope validation (F1)

- `record_event` (POST /api/v1/analytics/events) no longer persists
  caller-supplied identity/scope fields verbatim: `agent_id`/`user_id`
  are derived from the verified caller (body values accepted but
  ignored; impersonation sessions attribute to the impersonator), and
  `workspace_id`/`repo_id` are validated against the caller's tenant via
  the shared audit.rs `validate_workspace_scope`/`validate_repo_scope`
  helpers — cross-tenant attribution is Forbidden, unknown ids NotFound,
  so a forged event can never pollute another scope's filtered query
  results (the review's exact repro is pinned as a regression test).
  `session_id` stays caller-supplied (not an identity/scope field,
  matching the audit envelope).
- Regression tests: `record_event_binds_identity_to_caller_not_request_body`,
  `record_event_workspace_scope_validated_against_caller_tenant`,
  `record_event_repo_scope_validated_against_caller_tenant`; existing
  analytics tests authenticate via `build_router` + Bearer token now that
  the handler verifies the caller.

### Verification repair: rustfmt + clippy on changed lines

- Verification of candidate c5e9ffd failed `checks.sh` on two mechanical
  findings only (behavioral checks and the web build passed):
  `clippy::needless_borrow` at `spawn.rs:2128` (`&state` where `state` is
  already `&Arc<AppState>` — now passes `state` directly) and rustfmt
  drift on the seed closures of the analytics scope-filter/daily tests
  (now `cargo fmt`-clean).
- Re-verified in-sandbox with the verifier's own diff scripts:
  `check-rustfmt-diff.py` and `check-clippy-diff.py` clean against both
  the immediate parent (2 Rust files) and the assignment base 4a203a06
  (full 30-file task surface).

### Test evidence

- In-sandbox (this round, shared target dir via the build wrapper):
  `cargo clippy -p gyre-server --all-targets` finished clean on changed
  lines (full compile proof of lib + tests); `cargo fmt -- --check` clean;
  `check-rustfmt-diff.py` and `check-clippy-diff.py` clean against both
  HEAD^1 and assignment base 4a203a06; `cargo test -p gyre-domain
  analytics` (3 passed); `cargo test -p gyre-adapters analytics`
  (13 passed); all 18 scripts/check-*.sh mechanical gates pass.
- Server test binaries exceed this sandbox's 4 GiB build budget — the
  independent verifier must run:
  `cargo test -p gyre-server --lib api::analytics`,
  `cargo test -p gyre-server --lib api::audit`, and
  `cargo test -p gyre-server --test api_integration analytics`
  (evidence: /tmp/stage/review-evidence/task-146-verification-repair.md).
