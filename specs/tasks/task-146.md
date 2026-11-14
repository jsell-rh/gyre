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
commits: ["e5ee853a5259f68e5ed601dcb1d51eb905d524fe", "02e3945b63c2f28b8e65248d6331b3a3ca7cfce0", "fe61a49ffea2e7e0fe6341ebf261a6a9579ad79a", "08123b833c31d5c56fb62fa332caa58bb6a69257", "f17756774160de561cc1e0a9c1576a43e18b7505", "e1088342c4f52d501dc18f12b26bcb9a8b93987e", "9f68c1fed65784766f3f967de39cddc7e8466358", "a3afa55f6ff37e35473dc6f95b33811626510e3c", "b49cea80f1536251e1376a1b366aeb3cf0d7e93b", "9bf30a10b6db85a0a8bd8c29fe1c3d6a4378c4c0", "5fb9e9b83b1b8f30da3e87c4ad419267b8543092", "24b2d851a053d1da22c7e75db0a742bd903c0f7f", "e9e597a07dcbc28feadcce79500e7b845d030bb9", "f301cff2c388a93a12566692550cd4420f69e9db", "ad7d3e33a7e7ed0dd49c7bcad057e8d5be8a0494", "57d722692429f2433bae6790e8ceb56a139e40b3", "29d73e8102d4c0f55cd4cdcb28fa2a99f298ed2a", "02b8c9073a9fa0c153c17abadb23933bd492ea86", "6f469a6bbf65aa915005360bca99e2123c5d700e", "897b5565d8791abde8b81cf4de4fa26e3e23eda7", "17face906b7d075c68c798094f914b5d63488039", "3bcd6f5254589c02811ca2a902e98e22c06ab09b", "d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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
spec fields, the query API with the full spec filter set (`event_name`
exact / trailing-`*` literal prefix, agent/user/workspace/repo scope,
`since`/`until`, `limit`) plus server-side `group_by` aggregation, and
all 12 auto-emitted events recorded at their real trigger points with
the spec-required properties: `task.status_changed`, `mr.merged`,
`mr.closed`, `agent.spawned`, `agent.completed`, `agent.failed`,
`merge_queue.processed`, `gate.failed`, `gate.passed`, `spec.approved`,
`budget.warning`, `search.query`. Event identity is caller-bound and
scope fields are tenant-validated (no forged attribution).

### This round (merge task-130 base + review-repair)

Two pieces of work:

- **Merge 6ffd6799 (task-130 token scoping) into the candidate**
  (288d7296) — resolved four conflicts. Three were comment/note
  reconciliations where this branch's stricter text describes the merged
  code (workspace-orchestrator spawn keeps the no-system-bypass tenant
  containment pinned by `orchestrator_spawn_denied_across_tenants`;
  inert-enforcement-exemptions note kept for this tree's fix set). Two
  real semantic combinations:
  - `ralph_loop.rs` — task-130 moved the re-spawn JWT mint after persona
    resolution (hierarchy claims + persona slug + attestation level,
    fail-closed on unresolvable workspace); task-146's `persona_label`
    capture for the `agent.spawned` event was kept after it. Both sides'
    behavior present.
  - `mcp.rs` `handle_search` — combined task-146's caller-scope
    validation of the `workspace_id` arg (event-attribution integrity)
    with task-130's agent-JWT workspace-claim clamp, in that order.
  - Also fixed two analytics test call sites git could not adjust
    (task-146-only tests still calling the 4-arg
    `spawn_workspace_orchestrator_core`; task-130 added the
    `caller_tenant` parameter) and one comment the textual merge left
    describing a system-principal bypass the merged handler no longer
    has.
- **Review repair (b78a2494)** — recorded e5ee853a in task-195's
  `commits:` frontmatter per the reviewer's endorsed fix (the commit
  adapts task-195's `run_gate` surface); `check-task-commit-attribution`
  now exits 0 with the restored task-146 commit list. No exemption-file
  entries added.

Post-merge audit re-verified all 12 production emit sites, the 9-field
schema, and the full filter parameter set are intact on the merged tree.

### Test evidence

Run in this sandbox (focused probes; the build wrapper defers
gyre-server test execution to the independent verifier):

- `cargo check -p gyre-server --tests` — passes (merge resolution and
  both sides' test modules compile together; this probe caught and
  confirmed the fix for the 4-arg test call sites).
- `cargo test -p gyre-domain analytics` — 3 passed.
- `cargo test -p gyre-adapters analytics` — 16 passed (includes the
  literal/case-sensitive event_name prefix-filter regressions on
  SQLite + the grouped-count suites).
- `scripts/check-arch.sh`, `check-inert-enforcement.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-mcp-write-tools.sh`, `check-migration-versions.sh`,
  `check-byte-slice-truncation.sh`, `check-dead-message-kinds.sh`,
  `check-mem-port-contracts.sh`, `check-lossy-secret-conversion.sh`,
  `check-forwarded-header-trust.sh`, `check-unbounded-external-http.sh`,
  `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`,
  `check-in-memory-state-stores.sh`, `check-forged-scope-fields.sh`,
  `check-migration-sql-portability.sh` — all pass.
- `scripts/check-task-commit-attribution.sh` — passes with the restored
  task-146 commit list and the task-195 attribution repair (exit 0).

Server tests the verifier must execute (this round's conflict resolution
lives in ralph_loop/mcp/orchestrator and the gate_executor tests module):

- `cargo test -p gyre-server gate_executor` — task-146 analytics gate
  tests + task-195 manifest gate tests.
- `cargo test -p gyre-server orchestrator` — spawn containment,
  analytics persona/compute-target, and task-130 core-signature tests.
- `cargo test -p gyre-server ralph_loop` — re-spawn JWT hierarchy-claim
  behavior + `agent.spawned` re-spawn analytics.
- `cargo test -p gyre-server mcp` — search tool scope validation +
  agent-JWT workspace clamp.
- `cargo test -p gyre-server --test api_integration orchestrator_spawn_denied_across_tenants`
  — pins the no-bypass containment the merged comments describe.
- Plus the pre-existing analytics suites unchanged by this round.
