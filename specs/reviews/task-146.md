# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

Comparison base: 4dc0eb9bbd0a2cddad39f478b7bc70167fa455ed → candidate
d23cb064135aefa8ebfae6d50a9bb7148f4e1888.
Diff: 41 files, +5893/−370. The task's own commits are the 18 listed in
`specs/tasks/task-146.md` frontmatter at the candidate; the remainder of the
diff is rebase-reconciliation content merged in from upstream bases
(task-119 budget cascade, task-139 signed-approval spec policies, task-134
gate-executor fix), which this review treated as upstream state, not as
task-146 claims. Script-exemption edits in the diff remove entries and lower
frozen counts (never add), and all mechanical checks pass at the candidate
tree.

## Findings

No blocking findings. Evidence reviewed:

### Spec conformance (analytics.md §Event Schema)

- `AnalyticsEvent` (crates/gyre-domain/src/analytics.rs) carries all 9 spec
  fields; `with_scope` populates user/session/workspace/repo. Id-typed fields
  are stored as `Option<String>` — a documented representation choice (Id is a
  string newtype; storage columns are TEXT), pinned by the serde round-trip
  test `analytics_event_matches_spec_schema`. Probe:
  `cargo test -p gyre-domain analytics` → 3 passed.

### Auto-Emitted Events (all 12, real trigger points, all spec properties)

Verified in source at every site:

- `task.status_changed` — HTTP status endpoint (api/tasks.rs, via
  `record_task_status_changed` with old_status/new_status/task_id/assigned_to),
  MCP `update_task`, spawn auto-advance, completion auto-advance, admin
  force-kill, stale-abort, spec-reject cancel, repo-archive cancel, ralph-loop
  re-open/ceiling paths.
- `mr.merged` — HTTP transition (gate_count via `gate_results.list_by_mr_id`,
  queue_wait_secs via queue entry) and the merge-queue merge path
  (`emit_mr_merged`, deferred past rollback so no phantom merged events).
- `mr.closed` — HTTP transition, repo archival, spec-reject, ralph-loop
  gate-failure close; carries mr_id/repo_id/reason.
- `agent.spawned` — worker spawn (compute_target from
  `launch.compute_target_label`, persona from the bound persona row),
  orchestrator spawn (resolved compute target, not hardcoded "local"),
  ralph-loop re-spawn.
- `agent.completed` — `complete_agent` + MCP `gyre_agent_complete`
  (task_id, duration_secs from spawned_at).
- `agent.failed` — fail/stop endpoints, admin kill, stale-abort, compose
  teardown, repo-archive stop, worker failed-launch teardown
  (signal_chain.rs), ralph-loop ceiling/terminal paths; reason names the
  concrete cause.
- `merge_queue.processed` — merged outcome on both single and atomic-group
  paths; every terminal Failed path routed through `fail_queue_entry` so the
  status change and the event cannot diverge; Cancelled (circuit breaker and
  operator cancel) records outcome "skipped".
- `gate.failed`/`gate.passed` — `run_gate` in gate_executor.rs, with
  gate_id/gate_type/mr_id/output_snippet (truncated) and duration_secs.
- `spec.approved` — approval handler (spec_path, approver_type,
  approval_mode) with approver identity bound to agent/user id.
- `budget.warning` — `emit_budget_warning` on all three metric arms (agents,
  tokens, cost) of `check_spawn_budget` and the usage-recording paths;
  threshold 80% of limit, carries workspace_id/metric/threshold_pct.
- `search.query` — HTTP handler and MCP `gyre_search`, both through
  `record_search_query` with caller-bound identity and validated workspace
  scope.

Anti-forgery: `record_event` ignores caller-supplied agent_id/user_id and
derives them from the verified caller; workspace_id/repo_id are validated
against the caller's tenant (`validate_workspace_scope`/`validate_repo_scope`),
matching check-forged-scope-fields expectations for the new scope fields.

### Query API / Query Parameters

- `GET /api/v1/analytics/events` supports event_name (exact and trailing-`*`
  prefix via LIKE on SQLite and Postgres, starts_with on mem), agent_id,
  user_id, workspace_id, repo_id, since/until (ISO8601 or unix seconds, date
  forms extend to end-of-day as upper bound), limit (default 100, cap 10_000),
  group_by (event_name/agent_id/workspace_id/day; unsupported fields rejected).
- `GET /daily` supports the spec form `?event_name=&days=30`; until defaults
  to now.
- All three adapters (SQLite, Postgres, mem) implement `query_filtered` with
  matching semantics; migration `2026-10-08-000063_analytics_scope_columns`
  adds the four scope columns + indexes (next unused sequence number, portable
  SQL — both migration checks pass).
- No new routes were created (api/mod.rs untouched), per the task instruction.

### Probes (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-domain analytics` → exit 0.
- `cargo test -p gyre-adapters analytics` → exit 0 (13 passed, incl.
  `analytics_query_filtered_all_params`).
- `cargo check -p gyre-server` → exit 0.
- `cargo check -p gyre-server --tests` → exit 0 (all 22 per-event tests
  type-check).
- Mutation probe: removing the workspace_id predicate from sqlite
  `query_filtered` makes `analytics_query_filtered_all_params` FAIL (exit 1) —
  the filter test kills a dropped predicate; not self-confirming. Source
  restored and verified clean.
- Ten mechanical invariant scripts (migration versions/portability, mem port
  contracts, fabricated scope defaults, arch, byte-slice truncation, relative
  path defaults, lossy secret conversion, scope literal defaults, inert
  enforcement) all exit 0.

### Notes (non-blocking)

- Post-merge phase gates (`run_post_merge_gate_commands`) do not emit
  gate.passed/gate.failed; the spec properties are MR-keyed and the task plan
  scoped the events to "the merge processor gate evaluation loop", which is
  implemented (pre-merge `run_gate` records every gate result). Future task
  may revisit if post-merge gate outcomes should be tracked.
- `AnalyticsQueryFilter::event_name_like` in gyre-ports is currently unused
  (adapters inline the LIKE construction); harmless public helper.
- `GET /api/v1/analytics/events` does not tenant-scope results server-side —
  pre-existing behavior of the endpoint (unchanged by this task; the new
  workspace_id filter can only narrow). Out of this task's contract.

### Verifier must run (gyre-server test binaries too large for model budget)

- `cargo test -p gyre-server analytics`
- per-event tests listed in /tmp/stage/review-evidence/README.md (22 named
  tests across spawn/merge_requests/merge_processor/gate_executor/specs/
  budget/search/mcp/ralph_loop/stale_agents/admin/compose/repos/mem)
- `cargo test -p gyre-server --test api_integration`
- `cargo test --all` + `cd web && npm ci && npm test` (standard verification)
