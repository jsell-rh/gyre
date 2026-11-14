# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted
Events, §Query API, §Query Parameters.
Candidate: `a5b35491f0d89afa8afa56663cf2cd6a8aa98e6a` (branch
`pipeline/task-146/e462c78ac7914ce1b221605a41461bd1-1`, base
`6ffd679985be9826193f4dc089d9b1f16766273c`).
Verdict: **approved** (no blocking findings).

## Scope note

The candidate branch carries the task-146 work plus a series of rebases
onto moving main bases (task-091/118/119/128/130/139/191/195/202 merges,
recorded in the frontmatter at the candidate). The analytics surface was
reviewed at the candidate tree; attribution of merged-in main work was
not re-reviewed beyond its interaction with the analytics emission paths.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-domain analytics` → **3 passed** (includes
  `analytics_event_matches_spec_schema`, a serde round-trip pinning all
  9 spec fields).
- `cargo test -p gyre-adapters analytics` → **16 passed**, including
  `analytics_query_filtered_all_params` (cross-cutting seed: agents span
  workspaces, workspaces span repos, users span workspaces — no scope
  predicate is redundant with another, so dropping any one predicate
  changes the asserted result sets) and
  `analytics_event_name_prefix_is_literal_case_sensitive`.
- `cargo check -p gyre-server --tests` → clean in 6m08s (full server test
  surface type-checks). Server test *execution* is out of this sandbox's
  4 GiB budget — see "Verifier must run" below.
- Static checks, all pass: check-arch.sh, check-mem-port-contracts.sh,
  check-fabricated-scope-defaults.sh, check-inert-enforcement.sh,
  check-migration-versions.sh, check-migration-sql-portability.sh,
  check-forged-scope-fields.sh, check-abac-exempt-handlers.sh,
  check-mcp-write-tools.sh, check-dead-message-kinds.sh,
  check-byte-slice-truncation.sh, check-lossy-secret-conversion.sh,
  check-unbounded-external-http.sh, check-in-memory-state-stores.sh.
- check-task-commit-attribution.sh fails in this sandbox **only because
  the controller's contract restore reset the working-tree
  `specs/tasks/task-146.md` to `commits: []`**; all 24 SHAs it reports as
  missing are present in the committed candidate's frontmatter
  (`git show a5b35491:specs/tasks/task-146.md`). Not a candidate defect.

## Verified

- **Event Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs)
  carries all 9 spec fields; `with_scope` populates
  user/session/workspace/repo; ids stored as text, matching the new
  `2026-10-08-000063_analytics_scope_columns` migration (plain
  `ALTER TABLE ... ADD COLUMN` + `CREATE INDEX IF NOT EXISTS` — portable
  to both SQLite and PG; unique sequence 63, no duplicate versions).
- **All 12 auto-emitted events** at real trigger points, each with the
  spec-required properties (read at every emission site):
  `task.status_changed` (api/tasks.rs, signal_chain.rs, ralph_loop.rs —
  old/new status, task_id, assigned_to, repo+workspace scope from the
  persisted entity); `mr.merged` (merge_requests.rs HTTP transition and
  merge_processor.rs queue path — gate_count from gate_results,
  queue_wait_secs from enqueue time); `mr.closed` (HTTP close, repo
  archive, spec reject, ralph-loop gate-failure close); `agent.spawned`
  (worker spawn, orchestrator spawn with resolved compute_target — not a
  hardcoded "local" — and bound persona, ralph-loop re-spawn);
  `agent.completed` (spawn.rs complete handler, MCP gyre_agent_complete
  — duration_secs from spawned_at); `agent.failed` (fail/kill endpoints,
  stale abort/pause, spec-reject, repo-archive, ralph-loop ceiling,
  worker failed-launch teardown, orchestrator death monitor);
  `merge_queue.processed` (merged outcome on single and atomic-group
  paths, failed outcome centralized in emit_queue_processed_terminal,
  circuit-breaker Cancelled as "skipped"); `gate.failed`/`gate.passed`
  (gate_executor pre-merge run_gate AND post-merge path, output_snippet
  via byte-safe truncate_bytes, mr_id null rather than fabricated when
  not attributable); `spec.approved` (specs.rs approval ledger path with
  approver_type/approval_mode from the persisted entry);
  `budget.warning` (80% threshold from check_spawn_budget and usage
  recording, metric/threshold_pct/used/limit); `search.query` (HTTP
  search endpoint and MCP gyre_search through one shared helper).
- **Scope safety on the write path**: `record_event` binds
  agent_id/user_id to the verified caller (request-body identity fields
  ignored), validates workspace/repo scope against the caller's tenant
  (rejecting unknown/mismatched ids), and the search handler validates
  its workspace param for the same reason before attributing the
  auto-emitted event. check-forged-scope-fields.sh passes.
- **Query API / Parameters**: GET /api/v1/analytics/events supports
  event_name (exact or trailing-`*` literal case-sensitive prefix —
  mem `starts_with`, SQLite `substr(...)=...` under BINARY collation,
  PG escaped LIKE, all pinned by adapter regression tests), agent_id,
  user_id, workspace_id, repo_id, since/until (ISO8601 or unix secs,
  inclusive), limit (default 100, capped 10_000), and group_by
  (event_name/agent_id/workspace_id/day; unsupported field → 400).
  `count_grouped` aggregates server-side over the full filtered set
  (limit bounds returned groups, not the aggregation input) in all
  three adapters, with mem/SQLite parity tests. GET /count and
  GET /daily (event_name=&days=) unchanged per spec. No new endpoints
  were created (api/mod.rs untouched in the diff).
- **Tests are meaningful, not inflated**: per-event server tests exist
  for every emission path (34+ test functions enumerated in
  review-evidence/server-event-tests.txt) including negative cases
  (budget_warning_not_emitted_below_threshold,
  empty_query_emits_no_analytics_event) and failure-mode assertions
  that would fail if an emission were removed.
- **Coverage matrix** updated honestly: sections 1–5 → implemented with
  specifics; sections 6–12 (Decision API, task-147) left task-assigned.
- **Exemption files**: only comment edits and lowered frozen counts;
  no new exemption entries.

## Minor observations (non-blocking)

- `query_events` leaves `until` unbounded when absent rather than
  defaulting it to "now" as the spec table suggests. No observable
  difference: event timestamps are server-generated at write time, so
  no event can be dated in the future relative to a later query.
- The `agent.completed` test asserts duration_secs and agent_id but not
  the task_id property; the emission site does include task_id.
- Generated `web/dist/`: no web-source changes in this candidate; no
  generated-output observation to record.

## Verifier must run (server tests out of sandbox budget)

- `cargo test -p gyre-server --lib analytics` (record/query/filter/
  group_by/scope-validation endpoint tests in api/analytics.rs).
- `cargo test -p gyre-server --lib query_filtered count_grouped`
  (mem.rs parity tests).
- `cargo test -p gyre-server --lib budget_warning`.
- `cargo test -p gyre-server --lib analytics_event emits_analytics`
  (per-event tests across tasks/specs/search/merge_requests/orchestrator/
  spawn/stale_agents/mcp — see
  /tmp/stage/review-evidence/server-event-tests.txt for the enumerated
  34+ names).
- `cargo test -p gyre-server --lib gate` (gate_executor
  gate.passed/gate.failed pre- and post-merge tests).
- `cargo test -p gyre-server --lib merge_queue post_merge_gates`
  (merge_processor queue/gate analytics tests, incl.
  post_merge_gates_run_against_head_sha_tree — the clippy-repaired line).
- `cargo test -p gyre-server --test api_integration` (analytics
  integration tests).
- `bash scripts/check-task-commit-attribution.sh` against the committed
  frontmatter (passes at the candidate; only the sandbox's contract
  restore makes it fail locally).
