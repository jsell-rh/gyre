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
commits: ["44aa2ea6ada9f49090a7103a9fa54d7b511cfea2", "e5ee853a5259f68e5ed601dcb1d51eb905d524fe", "02e3945b63c2f28b8e65248d6331b3a3ca7cfce0", "fe61a49ffea2e7e0fe6341ebf261a6a9579ad79a", "08123b833c31d5c56fb62fa332caa58bb6a69257", "f17756774160de561cc1e0a9c1576a43e18b7505", "e1088342c4f52d501dc18f12b26bcb9a8b93987e", "9f68c1fed65784766f3f967de39cddc7e8466358", "a3afa55f6ff37e35473dc6f95b33811626510e3c", "b49cea80f1536251e1376a1b366aeb3cf0d7e93b", "9bf30a10b6db85a0a8bd8c29fe1c3d6a4378c4c0", "5fb9e9b83b1b8f30da3e87c4ad419267b8543092", "24b2d851a053d1da22c7e75db0a742bd903c0f7f", "e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06", "95177f7bcaf4568d94135f1d48cdcbaafd8537f1"]
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

The task-146 analytics surface: the `AnalyticsEvent` schema with all 9
spec fields (`agent_id`/`user_id`/`session_id`/`workspace_id`/`repo_id`
scope fields included), the query API with the full spec filter set
(`event_name` exact / trailing-`*` literal prefix, agent/user/workspace/
repo scope, `since`/`until`, `limit`) plus server-side `group_by`
aggregation, and all 12 auto-emitted events recorded at their real
trigger points with the spec-required properties: `task.status_changed`,
`mr.merged`, `mr.closed`, `agent.spawned`, `agent.completed`,
`agent.failed`, `merge_queue.processed`, `gate.failed`, `gate.passed`,
`spec.approved`, `budget.warning`, `search.query`. Event identity is
caller-bound and scope fields are tenant-validated (no forged
attribution). The branch also carries the merges of task-130 (6ffd6799,
token scoping) and task-056 (b6892403, abandoned-branch detection) with
conflict resolutions recorded in 288d7296 / 065e98e6 and the task-195
attribution repair b78a2494.

### This round (verification repair, 95177f7b)

The independent verifier's `cargo test --all` (run ad0c26e5, merged
head 065e98e6) failed exactly one test:
`stale_agents::tests::pause_records_agent_failed_analytics_event` —
`event must be scoped to the workspace for filtered queries: left
Some("default"), right Some("ws-2")`. Root cause is in the test setup,
not production: the test created workspace `ws-2` but never placed the
agent in it, so `Agent::new`'s bootstrap `workspace_id` ("default") was
the agent's real workspace, and `record_agent_failed` correctly scopes
the event to `agent.workspace_id`. The repair (95177f7b) assigns the
agent to `ws-2` in the test setup, making the assertion satisfiable and
meaningful — it now distinguishes "event follows the agent's actual
workspace" from a fabricated default-scope defect. No production code
changed in this round; all 2109 other tests in the run passed.

### Test evidence

- `cargo check -p gyre-server --lib` passes on the repaired head
  (this sandbox's build wrapper defers server test execution to the
  independent verifier; probes recorded in
  `/tmp/stage/review-evidence/task-146-stale-pause-repair.md`).
- `rustfmt --edition 2021 --check` on `stale_agents.rs` — clean.
- Verification run ad0c26e5 at the pre-repair head: 2109 passed, 1
  failed (only the test repaired above); all repository static checks
  passed (architecture, ABAC registry/exempt handlers, MCP write gates,
  migration versions/portability, dead message kinds, byte-slice
  truncation, relative path defaults, fail-open ref resolution, task
  commit attribution, mem port contracts, fabricated scope defaults,
  lossy secret conversion, scope literal defaults, inert enforcement,
  forged scope fields, forwarded header trust, in-memory state stores,
  unbounded external HTTP).

Server tests the verifier must execute (the repaired test plus the
analytics surface it belongs to):

- `cargo test -p gyre-server --lib stale_agents` — the repaired
  pause/abort agent.failed analytics tests.
- `cargo test -p gyre-server post_merge_gates_run_against_head_sha_tree`
  — post-merge gate.passed/gate.failed analytics on a real git repo.
- `cargo test -p gyre-server gate_executor` — task-146 analytics gate
  tests.
- `cargo test -p gyre-server orchestrator` — spawn containment,
  analytics persona/compute-target tests.
- `cargo test -p gyre-server ralph_loop` — re-spawn analytics.
- `cargo test -p gyre-server mcp` — search tool scope validation.
