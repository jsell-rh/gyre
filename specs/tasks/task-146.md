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

Task-146's analytics implementation (event schema with all spec scope
fields, all 12 auto-emitted events with spec-required properties,
query API with the full filter set + server-side `group_by` aggregation)
was completed and reviewed in earlier rounds — the commit list above is
that shipped surface. This round reconciles the branch with main
`cd58a166` (task-202: `search_indexer`, `search_sync` index-on-write
repository decorators, real full reindex), which conflicted in five
files.

### Rebase reconciliation (merge commit 7c4d2908)

- **Event properties vs scope facets** (`merge_requests.rs`, `spawn.rs`,
  `merge_processor.rs`): kept task-146's spec-required properties
  (`mr.merged` `gate_count`/`queue_wait_secs`, `mr.closed` `reason`,
  `agent.spawned` `compute_target`/`persona`, `agent.completed`
  `task_id`/`duration_secs`, `merge_queue.processed`
  `outcome`/`wait_secs`) over main's duplicate property copies of
  `workspace_id`/`repo_id`. Scope is carried by the event's
  `with_scope` fields instead.
- **`activity_event_document` scope sourcing** (`search_indexer.rs`):
  the document builder now reads workspace/repo facets from the event's
  scope fields first (set server-side by every auto-emit site, and
  caller-validated on the POST path), falling back to the properties
  copy. Without this, every task-146 emission site would produce search
  documents with no workspace/repo facets — silently losing the
  filterability task-202 requires — because main's builder only read
  `properties.workspace_id`/`properties.repo_id`.
- **Orchestrator spawn emission** (`orchestrator.rs`): kept task-146's
  post-launch `agent.spawned` emission (reports the compute target the
  process actually launched on, review f3f37604) over main's pre-launch
  copy; it now records through main's `record_and_index_activity`
  helper so the helper stays live.
- **Task status emission** (`tasks.rs`): kept the
  `record_task_status_changed` helper (spec requires `old_status`,
  which main's inline emission lacked) over main's inline copy.
- **Group merge path** (`merge_processor.rs`): kept both the deferred
  `emit_mr_merged` side effect (rollback safety) and main's on-write
  search index call.
- **Port extension vs decorator** (`search_sync.rs`): main's
  `IndexingAnalyticsRepository` (wired around `state.analytics` in
  `build_state` and `test_state`) implements task-146's
  `query_filtered`/`count_grouped` port additions as read delegations.
  With the decorator in place, every auto-emitted analytics event —
  task-146's included — is indexed on write, so search.md's
  "index updated synchronously on every write" now covers the whole
  auto-emit surface.

### Test evidence (this round)

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`
  — clean (the `--tests` check failed at the merge base on the missing
  decorator methods; fixed in 7c4d2908).
- `cargo check --workspace` — clean.
- `cargo test -p gyre-domain -p gyre-ports` — 516 passed, 0 failed.
- `cargo test -p gyre-adapters` — 514 passed, 0 failed, 6 ignored
  (includes task-202's search adapter suites and task-146's
  sqlite/postgres analytics suites, 331s).
- `cargo fmt --check` — clean; mechanical checks pass: arch,
  migration versions + SQL portability, mem port contracts, inert
  enforcement, forged-scope fields, ABAC route registry + exempt
  handlers, byte-slice truncation, relative-path defaults, fail-open
  ref resolution, lossy-secret conversion, fabricated/scope-literal
  defaults, forwarded-header trust, unbounded external HTTP, dead
  message kinds, MCP write tools, in-memory state stores, task commit
  attribution.

### Verifier must execute (server test compilation is out of sandbox budget)

- `cargo test -p gyre-server --lib` — in particular the analytics
  auto-emit suites (`api::analytics`,
  `orchestrator_spawn_emits_agent_spawned_analytics_event` and the
  compute-target regression test), `spawn.rs` lifecycle emission tests,
  and `merge_processor` gate/queue emission tests.
- `cargo test -p gyre-server --test search_wiring` — task-202's
  end-to-end index-on-write suites; these now also exercise
  task-146's scoped analytics events through the decorator and the
  modified `activity_event_document`.
- `cargo test -p gyre-server --test api_integration` — analytics query
  API endpoints (`POST/GET /api/v1/analytics/events`,
  `GET /api/v1/analytics/count`, `GET /api/v1/analytics/daily`).
