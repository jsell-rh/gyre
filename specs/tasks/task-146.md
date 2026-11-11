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
commits: ["02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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
  `since`, `until`, `limit` (default 100, cap 10_000), `group_by` —
  implemented on SQLite, Postgres, and mem adapters. The sqlite adapter
  test `analytics_query_filtered_all_params` seeds cross-cutting data
  (agents span workspaces, workspaces span repos, users span workspaces)
  and asserts each scope filter individually, so dropping any single
  predicate fails the test.
- Terminal-path completeness: a terminal queue-entry transition and its
  `merge_queue.processed` event cannot diverge (`fail_queue_entry`), and
  every task-status persistence records `task.status_changed`
  (`record_task_status_changed`) — covering terminal queue outcomes
  (quality-gate failure, MR/repo not found, non-mergeable MR during
  candidate selection and atomic-group checks, dep-health,
  spec/breaking-change/spec-link/constraint/attestation-ABAC blocks, git
  errors, circuit-breaker and operator-cancel `skipped`), internal task
  transitions (MCP `update_task`, spawn/completion auto-advance, admin
  force-kill, stale-abort, spec-rejection and repo-archive
  cancellation), and agent kill paths (fail, admin kill, stale abort,
  spec-rejection stop, repo-archive stop, compose teardown). The
  stale-agent Pause stop stays unemitted by design: pause is a
  configured graceful stop, not a failure or kill.
- The analytics scope-columns migration is renumbered past the base's
  colliding sequence — `2026-10-08-000060_analytics_scope_columns`;
  `check-migration-versions.sh` and the
  `no_duplicate_migration_version_prefixes` guard test both pass on the
  merged tree.

### Rebase onto task-091 base (merge 3c3086aef, fix 02b8c9073)

Resolved rebase repair 1887db69797b4108ad77fe90a1525d47 (base
cfeefbda). Both sides had independently fixed the GateFailure
"default" workspace fabrication in gate_executor.rs; kept the base's
task-134 rewrite shape and kept this branch's
`gate.passed`/`gate.failed` analytics emission, which auto-merged
cleanly above it. The two fabricated-scope script conflicts were
comment-only (same 5→1 lowering); narratives merged,
`FROZEN_EXEMPTION_COUNT` stays 1, and
`check-fabricated-scope-defaults.sh` passes on the merged tree. The
base's new `QualityGate.validation_type` field (migration
`2026-10-09-000059_gate_validation_type`) broke the branch-only test
`gate_failure_during_selection_records_queue_processed_failed`
(E0063, caught by `cargo check -p gyre-server --tests`); fixed by
setting `validation_type: None` like every other test literal
(02b8c9073).

### Contract-repair round (repair 2844c8be45d24eaba689edd5e8be265c)

The executor restored the assigned task document over the checkpoint
(acceptance checkboxes and frontmatter reset). No product-source change
was needed: the production fix above is preserved intact at the assigned
head, and this round only re-verified the merged tree and restored the
bookkeeping (progress, commits list, these Shipped notes) without
altering the protected contract text. Evidence:
/tmp/stage/review-evidence/contract-repair-2844c8be-verification.md.

### Test evidence

- This round (merged tree at head 6c0a0aa23): `cargo check -p
  gyre-server` and `cargo check -p gyre-server --tests` clean; `cargo
  test -p gyre-domain --lib analytics` 3 passed (incl.
  `analytics_event_matches_spec_schema`); `cargo test -p gyre-adapters
  --lib sqlite::analytics` 13 passed (incl.
  `analytics_query_filtered_all_params`); `cargo test -p gyre-adapters
  --lib migration` 7 passed (incl.
  `no_duplicate_migration_version_prefixes`); arch,
  migration-versions, migration-portability, and fabricated-scope
  checks exit 0. `check-task-commit-attribution.sh` passes once the
  restored commits list is in place (it flags the executor-restored
  `commits: []` frontmatter, not a source defect).
- Verifier-owned (4 GiB budget — recorded in
  /tmp/stage/review-evidence/contract-repair-2844c8be-verification.md):
  `cargo test -p gyre-server --lib analytics`, `--lib search`, `--test
  api_integration`, `--test search_wiring`; full workspace suite,
  all-target clippy, fmt, and GitHub CI.
- Prior rounds' full local evidence (mutation-verified adapter filters,
  431 adapter lib tests, 42 server analytics lib tests, 67
  api_integration tests, search_wiring) is in the commit history of the
  listed commits.
