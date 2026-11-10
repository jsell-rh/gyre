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
commits: ["d7641dddcc0ba8d5d3a7ff660d159b404cab4f54", "8f0ee1903e62beb75639ce30a7c8dc3bd4ee7e55", "ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
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

### Analytics event schema and auto-emitted events (commit 38c2c5e)

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields
  (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`,
  `repo_id`, `properties`, `timestamp`); `with_scope` populates the
  user/session/workspace/repo scope fields. A serde round-trip test pins
  the shape against the spec schema.
- All 12 spec-required auto-emitted events record at their real trigger
  points with all spec-required properties, each proven by a passing
  per-event test: `task.status_changed`, `mr.merged`, `mr.closed`,
  `agent.spawned`, `agent.completed`, `agent.failed`,
  `merge_queue.processed`, `gate.failed`, `gate.passed`, `spec.approved`,
  `budget.warning`, `search.query`.
- Query API: `POST/GET /api/v1/analytics/events`, `GET /count`, `GET /daily`
  with the full spec filter set — `event_name` (exact or trailing-`*`
  prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`,
  `until`, `limit` (default 100, cap 10_000), `group_by` — implemented on
  SQLite, Postgres, and mem adapters (adapter test
  `analytics_query_filtered_all_params`).

### Terminal-path completeness (review repair rounds b818d20d → 8f0ee1903)

Successive review rounds extended trigger coverage to every production
path, keeping the invariant that a terminal queue-entry transition and its
`merge_queue.processed` event cannot diverge (`fail_queue_entry`), and that
every task-status persistence records `task.status_changed`
(`record_task_status_changed`): terminal queue outcomes (quality-gate
failure, MR/repo not found, dep-health, spec/breaking-change/spec-link/
constraint/attestation-ABAC blocks, git errors, circuit-breaker
`skipped`), internal task transitions (MCP `update_task`, spawn/completion
auto-advance, admin force-kill, stale-abort, spec-rejection and
repo-archive cancellation), and agent kill paths (fail, admin kill,
stale abort, spec-rejection stop, repo-archive stop). The final repair
round (8f0ee1903, on merge bf39f38a2 of base 4e895f766) closed the last
three gaps found by review:

- **`merge_queue.processed` on the two remaining `Failed` transitions** —
  the candidate-selection non-mergeable-MR check (4b'') and the atomic-
  group R4 member non-mergeable check now route through
  `fail_queue_entry(..., "mr_not_mergeable", ...)`; previously both set
  `Failed` with no event while adjacent sites in the same code paths were
  wired.
- **`merge_queue.processed` on operator cancellation** — `cancel_entry`
  (DELETE /api/v1/merge-queue/:id) emits outcome `skipped` via
  `emit_queue_processed_terminal`, same as the circuit-breaker Cancelled
  path.
- **`agent.failed` on the compose teardown kill path** — teardown emits
  `agent_id`/`task_id`/`reason` ("compose session torn down")/
  `duration_secs` for every session agent it marks Dead. (The stale-agent
  Pause stop stays unemitted by design: pause is a configured graceful
  stop, not a failure or kill.)

Each fix has a regression test that fails on the old code:
`non_mergeable_mr_during_selection_records_queue_processed_failed`,
`atomic_group_non_mergeable_member_records_queue_processed_failed`,
`cancel_records_queue_processed_skipped`, and assertions added to
`compose_teardown_stops_all_agents` (probe run with emissions reverted:
4 failures; with fixes: 0).

### Merge and gate hygiene across rounds

- Reconciled with main/base five times (d81b0fe4, a488d9df; 4118553f1,
  28fe73a7f; 4e895f766, bf39f38a2) — conflicts resolved as unions, never
  dropping a fix or re-adding an exemption. The bf39f38a2 merge unioned
  admin.rs (analytics `agent.failed` + task-103 token revocation/lifecycle
  audit) and stale_agents.rs tests, and intersected the fabricated-scope
  exemption fixes: auth.rs fixed by task-103, gate_executor.rs fixed by
  this branch → entries 3 → 1, FROZEN_EXEMPTION_COUNT lowered 3 → 1.
- Migration collisions with base lineages (Diesel runs one migration per
  version) were renumbered to the next unused sequence — currently
  `2026-10-08-000059_analytics_scope_columns`.
- Fixed for real, never via exemptions: orchestrator spawn tenant
  containment, attestation-ABAC merge enforcement, `warn_unapproved_persona`
  rename, char-boundary-safe SHA truncation, spec tenant fanout resolution,
  GateFailure scope resolution, fail-closed `resolve_ref` sites, the
  relative-path literal in spawn.rs, and the dead traces.rs exemption
  entry. `rust_extractor.rs` `never_loop` rewritten for the CI clippy gate.
- web/dist is byte-identical to base (this branch never modified it).

### Test evidence (this round: merge bf39f38a2 + commit 8f0ee1903)

- `cargo test -p gyre-server --lib`: 1517 passed, 0 failed;
  `cargo test -p gyre-server --test api_integration`: 67 passed, 0 failed;
  `cargo test -p gyre-adapters --lib`: 384 passed, 0 failed; `cargo fmt
  --check` clean; `cargo clippy -p gyre-server --lib` exit 0 (warnings
  only, pre-existing).
- All branch-invariant `scripts/check-*.sh` gates pass (migration
  versions/portability, fabricated-scope, scope-literal, relative-path,
  inert-enforcement, abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, arch, task-commit-attribution, and the rest).
- Checks that fail identically on base 4e895f766 (verified in a detached
  worktree at the base commit: 22 mawk-dialect awk errors and the
  pre-existing exit-1 violation sets) are documented in
  /tmp/stage/review-evidence/test-evidence.md — not introduced by this
  branch.

### Contract-repair round: Shipped-heading restore and base fefde2946 merge (this round)

The controller's checkpoint comparison flagged a protected-text change:
the previous checkpoint's task document had lost the `## Shipped`
heading — its round subheadings followed `## Agent Instructions`
directly, leaving every progress note outside the required section. The
executor restored the assigned task document; this round rebuilt it with
the heading intact and every note under `###` subheadings, and re-added
`8f0ee1903` to the `commits:` frontmatter (the task-labeled
product-surface fix commit) so `check-task-commit-attribution.sh` passes
with no exemption.

The branch was also merged with base fefde2946 (the task-089 lineage:
HSI Explorer Generated Views). The merge is additive for this task's
surface: `merge_requests.rs` gained only the `has_trace`/`limit`
workspace-MR-list filter and its tests; the `mr.closed` analytics
emission and every other task-146 trigger point are byte-identical.
No migration collision this round (base added none; the analytics
migration stays `2026-10-08-000059_analytics_scope_columns`).

Verified on the merged HEAD: every task-146 regression test passes —
`cancel_records_queue_processed_skipped`,
`non_mergeable_mr_during_selection_records_queue_processed_failed`,
`atomic_group_non_mergeable_member_records_queue_processed_failed`,
`gate_failure_during_selection_records_queue_processed_failed`,
`mr_not_found_during_selection_records_queue_processed_failed`,
`spec_policy_block_records_queue_processed_failed`,
`circuit_breaker_cancel_records_queue_processed_skipped`,
`spawn_auto_advance_records_task_status_changed`,
`complete_auto_advance_records_task_status_changed`,
`abort_records_agent_failed_analytics_event`, and
`compose_teardown_stops_all_agents` — plus the full
`merge_processor::tests`, `api::analytics`, `api::merge_queue`,
`api::compose::tests`, and `api::merge_requests::tests` suites
(178 tests, 0 failed). `check-task-commit-attribution.sh`,
`check-migration-versions.sh`, `check-arch.sh`,
`check-fabricated-scope-defaults.sh`, and
`check-relative-path-defaults.sh` all exit 0. web/dist is byte-identical
to base (this branch never modified it). Evidence retained under
/tmp/stage/review-evidence.

### Verification-repair round: deterministic rate-limit exhaustion test (commit d7641dddc)

The verification `cargo test --all` run on candidate 2c8b65581 failed
in `middleware::tests::rate_limit_exhaustion_returns_429` (assertion
left: 200, right: 429). Root cause was a wall-clock race in the test,
not a limiter defect and not a task-146 regression: the test predates
this branch (03b65d7dc), and `RateLimiter` refills the bucket to the
full rate whenever the wall clock crosses a second boundary, so when
the test's two back-to-back requests straddle that boundary — likely
under full-suite parallel load — the second request draws a refilled
token and returns 200.

Reproduced on the unmodified candidate binary: 4 failures / 200 runs
under 4-way CPU load, 0 / 300 unloaded; captured output matches the
verification failure exactly. The fix injects the clock into
`RateLimiter` (`with_clock`); the exhaustion test now freezes the clock
so no refill can occur between its two requests, and the refill branch
gains its first deterministic unit test
(`refills_to_full_rate_each_second`) plus a frozen-clock guard test.
Assertion strength is unchanged (first request 200, second 429). Probe
evidence under /tmp/stage/review-evidence: 0 failures / 300 runs under
the same load post-fix; `rate_limit` and `middleware::` suites pass
(24 + 19 tests), `cargo fmt --check` clean, clippy clean on changed
lines.

### Rebase-repair round: base b5a872087 merge (merge commit 72da380b3)

Resolved all 12 conflicted files from the controller-initiated merge of
base `b5a872087` (task-108 lineage: admin impersonation, task-099 F3
launch outcomes, task-104 ABAC-denied auditing, task-106 jj workspace
layout, task-168 git_http scope fix, secret-rotation teardown hooks)
into this branch. Unions wherever both sides fixed different things:

- `agent.failed`/`agent.completed` analytics emissions (compose
teardown, repo archive, fail_agent, MCP completion) kept alongside
base's `revoke_agent_task_secrets` teardown at the same terminal paths.
- `orchestrator.rs`: base's fail-closed `resolve_orchestrator_persona`
adopts in place of this branch's advisory `warn_unapproved_persona`
(stronger fix of the same inert-enforcement finding); base's launch
outcome 3-tuple cores and system-principal tenant bypass adopted; this
branch's `agent.spawned` emission and regression test kept (adapted to
the 3-tuple) alongside base's `spawn_reports_truthful_launch_outcome`.
- `rate_limit.rs`/`middleware.rs`: both lineages fixed the same 429
test race independently — base's boxed `NowSecsFn` clock adopted, this
branch's limiter-injection test refactor kept, both deterministic tests
kept.
- fabricated-scope exemptions intersected: auth.rs (task-103/104),
gate_executor.rs (this branch), git_http.rs (task-168, base) are all
fixed in the merged tree, so only speculative_merge.rs:372 remains and
FROZEN_EXEMPTION_COUNT lowers to 1 (below both parents).
- `specs/coverage/SUMMARY.md` rows unioned and TOTAL recomputed from
the merged table (802 / 220 n/a / 300 assigned / 119 implemented /
163 verified, 48%).

Offline gates all exit 0 on the merge commit (arch, ABAC registry +
exempt handlers, MCP write tools, dead message kinds, byte-slice,
relative-path, fail-open refs, scope literals, lossy secrets, inert
enforcement, mem port contracts, forwarded headers, in-memory state,
unbounded HTTP, SQL portability, forged scope fields, migration
versions, task attribution, fabricated-scope). `cargo fmt --check`
clean. All 12 spec-required analytics events retain live production
emitters post-merge. web/dist byte-identical to base.

`cargo test` could not run in this sandbox: crates.io egress is blocked
at the egress proxy (only github.com/api.github.com/registry.npmjs.org
reachable; no vendored crates or registry cache). This is a transport
restriction recorded for remote verification, not a code defect — full
suite, clippy, and CI are deferred to the deterministic verification
stage. Details and the per-file resolution ledger: /tmp/stage/review-
evidence/test-evidence.md.
