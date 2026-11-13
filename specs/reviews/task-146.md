# Review — task-146 (analytics §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events,
§Query API, §Query Parameters. Candidate `b2abfb5ec6768e79da92f188bd7fe78993d80b79`
against base `23d7361f2c2782cdd51b56011c47f3b11a6d3b86` (the branch additionally
carries the rebase-repair merge `b57cfc60` reconciling with main's task-119 budget
enforcement cascade — the earlier admin.rs/budget.rs rebase conflict recorded in the
durable findings is resolved in-tree; `cargo check -p gyre-server` compiles clean).
Verdict: **complete** — all five assigned coverage sections are backed by real
production code.

## Probes run (this review, /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain -- analytics` → **3 passed** (incl.
  `analytics_event_matches_spec_schema`, a serde round-trip pinning all 9 spec
  fields plus `with_scope` population).
- `cargo test -p gyre-adapters -- sqlite::analytics` → **13 passed**
  (query_filtered semantics — prefix wildcard, conjunctive scope filters,
  inclusive since/until bounds, limit — against real SQLite storage, plus the
  mem-adapter parity tests and `analytics_query_filtered_all_params`).
- `cargo check -p gyre-server` → clean (6m16s; server test compilation is left
  to the independent verifier per the 4 GiB budget directive — see list below).
- All 18 mechanical invariant scripts pass on the candidate tree:
  arch, migration-versions, abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, dead-message-kinds, byte-slice-truncation,
  relative-path-defaults, fail-open-ref-resolution, mem-port-contracts,
  fabricated-scope-defaults, scope-literal-defaults, lossy-secret-conversion,
  inert-enforcement, forged-scope-fields, forwarded-header-trust,
  in-memory-state-stores, unbounded-external-http, migration-sql-portability.
  The exemption-file edits only lower frozen counts (5→1) and reword comments —
  no new exemptions were added.
- Migration `2026-10-08-000063_analytics_scope_columns` (up/down, portable
  ALTER TABLE + CREATE INDEX IF NOT EXISTS) uses the next unused sequence for
  its date-prefix; `check-migration-versions.sh` OK (no duplicate Diesel
  versions).

## Verified working (inspection of every emit site against the spec table)

- **Event Schema** — `AnalyticsEvent` (gyre-domain) carries all 9 spec fields;
  `with_scope` populates user/session/workspace/repo. The id-typed fields are
  `Option<String>` rather than the spec sketch's `Option<Id>`, but `Id` is a
  transparent `String` newtype (gyre-common/src/id.rs:6), so the wire/storage
  shape, filtering, and round-trip are identical; the deviation is documented
  in the coverage row. Non-blocking observation, not a behavioral gap.
- **All 12 auto-emitted events** emit at real trigger points with all
  spec-required properties, and each has a per-event integration test that
  drives the real router/handler and asserts the recorded event via the
  analytics port (a removed emit site fails the `events.len() == 1` assert):
  - `task.status_changed` — HTTP status endpoint (old_status captured before
    mutation), MCP update_task, spawn/completion auto-advance, admin
    force-kill, stale-abort, spec-reject cancel, repo-archive cancel,
    ralph-loop re-open and iteration-ceiling blocks.
  - `mr.merged` — HTTP transition (gate_count from real gate_results,
    queue_wait_secs from queue entry) and the merge-queue path (deferred past
    atomic-group rollback so no phantom merged events).
  - `mr.closed` — transition endpoint, repo archival, spec-reject,
    ralph-loop gate-failure close; `reason` names the concrete cause.
  - `agent.spawned` — worker spawn and orchestrator spawn, both reporting the
    launch's resolved compute_target and the bound persona (regression tests
    pin both; orchestrator task_id is honestly null, not fabricated).
  - `agent.completed` — HTTP complete and MCP gyre_agent_complete, with
    duration_secs from spawned_at.
  - `agent.failed` — fail/stop endpoints, admin kill, stale abort,
    spec-reject stop, repo-archive stop, ralph-loop ceilings, worker
    failed-launch teardown, orchestrator process-death monitor.
  - `merge_queue.processed` — merged path plus every terminal
    Failed/Skipped path (selection-time gate failure, MR/repo not found,
    non-mergeable, spec policy, atomic-group rollback, circuit-breaker
    cancel → "skipped"), each with outcome/wait_secs.
  - `gate.failed`/`gate.passed` — gate_executor after real gate execution,
    with output_snippet (char-boundary-safe 512-byte truncate) and
    duration_secs respectively; scope resolved from the MR row, skipped when
    the MR no longer resolves (no fabricated "default" workspace).
  - `spec.approved` — specs.rs approval handler with spec_path,
    approver_type, approval_mode from the ledger entry.
  - `budget.warning` — check_spawn_budget and usage-recording paths, all
    three metrics (agents/tokens/cost), 80% threshold; the below-threshold
    negative test proves the threshold is real, not always-emit.
  - `search.query` — HTTP search and MCP gyre_search, with query_length
    (char count), entity_types, result_count, duration_ms; empty query
    emits nothing (negative test).
- **Query API / Parameters** — no new endpoints (task constraint respected;
  api/mod.rs route set unchanged from base). `GET
  /api/v1/analytics/events` supports event_name (exact or trailing-`*`,
  LIKE on both SQLite and Postgres), agent_id, user_id, workspace_id,
  repo_id, since/until (ISO8601 or unix secs; date-only upper bound extends
  to end-of-day), limit (default 100, cap 10_000), and group_by
  (event_name/agent_id/workspace_id/day, unsupported field → 400). `/daily`
  supports the spec's `?event_name=&days=30` form. The inbound
  `record_event` binds agent/user identity to the verified caller (request
  body fields ignored) and validates workspace/repo scope against the
  caller's tenant (Forbidden/NotFound on mismatch) — with dedicated
  anti-forgery tests; the search handler applies the same rule to its
  workspace_id param because it attributes the search.query event.
- **Coverage matrix** honestly updated: rows 1–5 implemented with accurate
  notes; Decision API rows remain task-assigned to task-147; 0 not-started.
  docs/api-reference.md updated for the search scope-validation and
  search.query emission.

## Server tests the independent verifier must run

```
cargo test -p gyre-server --lib -- analytics emits records_
cargo test -p gyre-server --lib -- budget_warning merge_queue queue_merge closing_mr merging_mr approve_spec update_task task_status orchestrator_spawn
cargo test -p gyre-adapters -- sqlite::analytics postgres::analytics   # (sqlite ran here: 13 passed)
cargo test -p gyre-domain -- analytics                                  # (ran here: 3 passed)
```

Key names: `spawn_emits_agent_spawned_analytics_event`,
`spawn_analytics_persona_reports_bound_persona`,
`complete_agent_emits_analytics_event`, `fail_agent_emits_analytics_event`,
`passing_gate_emits_gate_passed_analytics_event`,
`failing_gate_emits_gate_failed_analytics_event`,
`queue_merge_records_mr_merged_analytics_event`,
`closing_mr_emits_analytics_event`,
`merging_mr_emits_analytics_event_with_gate_count`,
`approve_spec_emits_analytics_event`,
`budget_warning_emitted_when_threshold_crossed`,
`budget_warning_not_emitted_below_threshold`,
`search_emits_analytics_event`, `empty_query_emits_no_analytics_event`,
`gyre_search_records_search_query_analytics_event`,
`agent_complete_records_agent_completed_analytics_event`,
`abort_records_agent_failed_analytics_event`,
`update_task_records_status_changed_analytics_event`,
`task_status_transition_emits_analytics_event`,
`record_event_binds_identity_to_caller_not_request_body`,
`record_event_workspace_scope_validated_against_caller_tenant`,
`record_event_repo_scope_validated_against_caller_tenant`,
`query_events_group_by_aggregates`, `query_events_since_until_accept_iso8601`,
`query_events_event_name_prefix_wildcard`, `query_events_filters_by_scope_ids`,
plus the merge_processor terminal-path
`*_records_queue_processed_failed` / `*_records_queue_processed_skipped` set.

## Minor (non-blocking, recorded for completeness)

- Id-typed scope fields are `Option<String>` (transparent newtype; see above).
- `spec.approved` is emitted only when the ledger entry exists and
  `current_sha` matches the approved SHA — approvals for unregistered spec
  paths would not record; the approve flow requires a pushed/registered spec,
  so this is unreachable in practice.
- `budget.warning` fires on every check once ≥80% (no per-crossing
  deduplication) — the spec says "threshold crossed" without rate limiting;
  event volume, not correctness.
- Generated `web/dist/` was not rebuilt by the agent — owned by the verifier;
  no authored frontend changes are part of this task.
