# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

Comparison base: 4ec1e52658f80e009292c1402e158b48ff1b1e88 → candidate a5ec8f7809de6ef8f04a8d5c070d34b5a98afb40.
Diff: 41 files, +5968/−370. The branch carries many rebase/reconciliation merges
onto evolving bases (task-091/093/118/119/122/134/139/168/193 lines) plus prior
review-repair rounds; the reviewed tree is the final merged state.

## Findings

No blocking findings. Evidence reviewed:

### Acceptance criterion 1 — AnalyticsEvent struct matches spec schema

`gyre-domain/src/analytics.rs`: all 9 spec fields present (`id`, `event_name`,
`agent_id`, `user_id`, `session_id`, `workspace_id`, `repo_id`, `properties`,
`timestamp`). `with_scope()` populates the four scope fields (Id inputs
stringified — the storage layer keeps TEXT columns; `Option<String>` vs
`Option<Id>` is a representation choice consistent with the adapters, and the
serde round-trip test `analytics_event_matches_spec_schema` pins every field
including the scope fields). Ran it: passes (504 domain tests, exit 0).
SQLite/Postgres rows and the mem adapter carry the same columns; migration
`2026-10-08-000063_analytics_scope_columns` adds them with indexes, uses
portable SQL only (portability check passes), and 000063 is the unique next
sequence (duplicate-version check passes).

### Acceptance criterion 2 — all 12 auto-emitted events at trigger points

Every event name has a production (non-test) emission site; each was inspected:

- `task.status_changed` — `api/analytics.rs::record_task_status_changed`,
  called from the HTTP status endpoint, MCP `update_task`, spawn/complete
  auto-advance, admin kill, stale-abort, spec-reject cancel, repo-archive
  cancel, and ralph-loop re-open/iteration paths (grep-verified call sites).
- `mr.merged` — HTTP transition (merge_requests.rs) and the merge queue
  (`emit_mr_merged`, deferred until after the rollback-capable loop so a group
  rollback cannot leave a phantom event).
- `mr.closed` — HTTP transition, repo-archive close, spec-reject close, and
  ralph-loop gate-failure close; each carries `reason`.
- `agent.spawned` — worker spawn (spawn.rs), orchestrator spawn
  (orchestrator.rs, compute target from the actual launch outcome), and
  ralph-loop re-spawn.
- `agent.completed` — HTTP complete (spawn.rs) and MCP `gyre_agent_complete`.
- `agent.failed` — fail/stop endpoints, admin kill, stale-abort, spec-reject
  stop, repo-archive stop, ralph-loop ceiling/terminal failure, worker
  failed-launch teardown, orchestrator process-death monitor, agent status
  endpoint.
- `merge_queue.processed` — merged path plus every terminal failure path via
  `emit_queue_processed_terminal` (gate failure in selection, MR/repo missing,
  non-mergeable, spec policy, breaking-change policy, dependency closed,
  attestation-ABAC denial) and circuit-breaker Cancelled as `outcome:
  "skipped"`.
- `gate.failed`/`gate.passed` — gate_executor.rs after each gate execution,
  keyed on actual GateStatus.
- `spec.approved` — specs.rs approve handler, approver identity resolved from
  the verified approval event.
- `budget.warning` — `emit_budget_warning` on all three metric arms of the
  spawn-budget check (80% threshold, positive and negative tests).
- `search.query` — both full-text search paths: HTTP search handler and the
  MCP `gyre_search` tool, through the shared `record_search_query` helper.

### Acceptance criterion 3 — spec-required properties

Each emission site was read; all spec-required properties are present
(`old_status`/`new_status`/`task_id`/`assigned_to`; `gate_count`/`queue_wait_secs`;
`reason`; `compute_target`/`persona`; `duration_secs`; `outcome`/`wait_secs`;
`gate_id`/`gate_type`/`output_snippet`; `spec_path`/`approver_type`/
`approval_mode`; `workspace_id`/`metric`/`threshold_pct`; `query_length`/
`entity_types`/`result_count`/`duration_ms`). Honest-value details: persona
reports the bound persona slug (regression test
`spawn_analytics_persona_reports_bound_persona` guards against the hardcoded
label), orchestrator `task_id` is null rather than a fabricated id, and
compute_target reports the launched target, not a hardcoded "local".

### Acceptance criterion 4 — Query API filter parameters

`query_filtered` on the port is implemented by all three adapters (SQLite,
Postgres, mem) with identical semantics: event_name exact or trailing-`*`
prefix, agent/user/workspace/repo scope ids, since/until (inclusive), limit.
The HTTP GET handler accepts ISO8601 or unix-seconds since/until, limit with
10_000 cap, and `group_by` (event_name/agent_id/workspace_id/day, unsupported
fields rejected). `count` and `daily` (with the spec `?event_name=&days=`
form) unchanged in shape. Verified against real SQLite rows:
`analytics_query_filtered_all_params` passes, and a mutation probe (workspace
filter disabled in the adapter) makes it FAIL — the test is not
self-confirming. Mutation restored; suite re-passed.

### Security/scope handling on new code paths

- `record_event` (POST) ignores caller-supplied agent/user identity and binds
  to the verified caller; workspace/repo attribution validated against the
  caller's tenant (Forbidden/NotFound on mismatch). Tests cover all three.
- `search.query` emission attributes the caller-validated workspace scope;
  the search handler's `workspace_id` param is validated up front precisely
  because it doubles as event attribution.
- No new routes were added (task constraint respected); ABAC registry and
  exempt-handler checks pass unchanged.
- Exemption-file changes are comment-only (counts lowered, no new entries).
- All 19 mechanical invariant scripts pass on the candidate tree
  (see /tmp/stage/review-evidence/probe-mechanical-checks.txt).

### Test quality

Per-event tests assert exact event counts and individual properties through
real entry points (axum router oneshots, MCP tool calls, real queue
processing), so removing an emission or a property fails a test. Negative
tests exist (no budget.warning below threshold, failed gate emits no
gate.passed, empty query emits no search.query). The mem adapter filter test
pins parity with SQLite semantics.

### Probes run in this review (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-domain` → 504 passed (incl. spec-schema test).
- `cargo test -p gyre-adapters --lib sqlite::analytics` → 13 passed.
- Mutation probe on sqlite `query_filtered` workspace filter → test fails;
  restored → passes.
- `cargo check -p gyre-server` → 0 errors (7m47s; server test compilation
  deferred to the verifier per the 4 GiB budget rule).
- 19 `scripts/check-*.sh` invariant lints → all pass.

### Verifier must execute (server tests not run in this sandbox)

- `cargo test -p gyre-server --lib` (full lib; minimally the analytics
  per-event tests): api::analytics::tests::* (record/query/filter/group_by/
  daily/scope-validation), spawn_emits_agent_spawned_analytics_event,
  spawn_analytics_persona_reports_bound_persona,
  spawn_auto_advance_records_task_status_changed,
  complete_agent_emits_agent_completed_event,
  agent_complete_records_agent_completed_analytics_event,
  fail_agent_emits_analytics_event, abort_records_agent_failed_analytics_event,
  task_status_transition_emits_analytics_event,
  update_task_records_status_changed_analytics_event,
  closing_mr_emits_analytics_event,
  merging_mr_emits_analytics_event_with_gate_count,
  queue_merge_records_mr_merged_analytics_event,
  reject_spec_records_mr_closed_analytics_events,
  gate_failure_during_selection_records_queue_processed_failed (and the other
  queue-processed terminal tests at merge_processor.rs:5449–5792),
  passing_gate_emits_gate_passed_analytics_event,
  failing_gate_emits_gate_failed_analytics_event,
  approve_spec_emits_analytics_event,
  budget_warning_emitted_when_threshold_crossed,
  budget_warning_not_emitted_below_threshold,
  search_emits_analytics_event,
  gyre_search_records_search_query_analytics_event,
  orchestrator_spawn_emits_agent_spawned_analytics_event,
  orchestrator_spawn_analytics_reports_resolved_compute_target.
- `cargo test -p gyre-server --test api_integration` (analytics additions).
- `cargo test --all` + `cd web && npm ci && npm test` per repo standard.

### Notes (non-blocking)

- N1 (environment, not a code defect): the working tree arrived with an
  uncommitted harness-side reset of `specs/tasks/task-146.md` to the pristine
  requirement input (`progress: not-started`, `commits: []`). The candidate
  commit a5ec8f78 itself contains the correct bookkeeping (progress
  ready-for-review, 19-commit attribution list matching every task-labeled
  work commit in 4ec1e526..a5ec8f78, and a detailed Shipped section). This
  review did not modify the file.
- N2: `mr.merged` from the HTTP transition endpoint computes `queue_wait_secs`
  only when the MR is still present in the live queue list; for an MR merged
  after its queue entry was consumed the property is null. Spec says
  "queue_wait_secs" without mandating non-null; the queue path (the primary
  merge path) always supplies it. Acceptable.
- N3: analytics `tenant_id` column is written as the literal `"default"` on
  insert in SQLite/Postgres adapters — pre-existing base behavior (present at
  4ec1e526), not introduced by this candidate.

## Verdict

No findings — task meets analytics.md §Event Schema / §Auto-Emitted Events /
§Query API / §Query Parameters with real implementations and kill-capable
tests. Approve.
