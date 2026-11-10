# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Assigned base `18adceb68`, candidate `c2ae50689` (branch `pipeline/task-146/305455f30c6a4380acb5ab466679e323-1`).
Verdict: **complete (approved)** — all five acceptance criteria hold with independent probe evidence; no findings.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain --lib analytics` → 3 passed (incl. `analytics_event_matches_spec_schema`).
- `cargo test -p gyre-adapters --lib analytics` → 13 passed (incl. `analytics_query_filtered_all_params`).
- `cargo test -p gyre-server --lib analytics` → 42 passed; `--lib budget` 13 passed; `--lib search` 3 passed.
- `cargo test -p gyre-server --test api_integration` → 67 passed / 0 failed.
- Offline gates, each exit 0: arch, migration-versions, abac-route-registry, abac-exempt-handlers, forged-scope-fields, fabricated-scope-defaults, scope-literal-defaults, inert-enforcement, mem-port-contracts, task-commit-attribution, mcp-write-tools, unbounded-external-http.
- **Independent mutation probes (both restored; tree verified clean afterward):**
  - Disabling the `user_id` predicate in `SqliteAnalyticsRepository::query_filtered` → `analytics_query_filtered_all_params` FAILS ("user u1 spans ws-1 and ws-3") — the strengthened adapter test genuinely kills a dropped filter (`mutation-user-filter.log`).
  - Renaming the `gate.passed` emission in `gate_executor::run_gate` → `passing_gate_emits_gate_passed_analytics_event` FAILS ("one gate.passed event expected") — the per-event test kills a disabled emission (`mutation-gate-passed-emission.log`).

## What was verified

- **Event Schema (AC1):** `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields; `with_scope` populates user/session/workspace/repo; serde round-trip test pins the shape. Migration `2026-10-08-000060_analytics_scope_columns` adds the four scope columns (renumbered past base's colliding 000059 — `check-migration-versions.sh` exit 0, no duplicate prefixes; SQL is SQLite/PG-portable: `ADD COLUMN`, `CREATE INDEX IF NOT EXISTS`).
- **All 12 auto-emitted events at real trigger points (AC2):** emit sites inspected for every event — `task.status_changed` (`record_task_status_changed`, called from tasks/mcp/spawn/admin/specs/repos/stale_agents), `mr.merged` (HTTP transition + queue path via `emit_mr_merged`, deferred past atomic-group rollback), `mr.closed` (transition / repo-archive / spec-reject), `agent.spawned` (worker + orchestrator tiers), `agent.completed` (HTTP completion + MCP `gyre_agent_complete`), `agent.failed` (fail / admin kill / stale-abort / spec-reject stop / repo-archive stop / compose teardown), `merge_queue.processed` (merged outcome in both single and atomic-group paths, plus every terminal Failed path via `fail_queue_entry`/`emit_queue_processed_failed`/direct conflict & git-error paths, and `skipped` for circuit-breaker/operator-cancel), `gate.failed`/`gate.passed` (`run_gate`, covering all GateTypes), `spec.approved`, `budget.warning` (spawn check + usage recording), `search.query` (real handler, post-execution).
- **Spec-required properties (AC3):** each emit site's JSON inspected against the spec table — all present, including `old_status`/`new_status`/`assigned_to`, `gate_count`/`queue_wait_secs`, `output_snippet` (char-boundary-truncated), `compute_target`/`persona`, `outcome`/`wait_secs`, `approver_type`/`approval_mode`, `threshold_pct`, `entity_types`/`result_count`/`duration_ms`. Multi-path emissions (e.g. `reason` for mr.closed) carry path-accurate values, not placeholders.
- **Query API + parameters (AC4):** `GET /api/v1/analytics/events` supports `event_name` (exact and trailing-`*` prefix via LIKE on sqlite/postgres, prefix-match on mem), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs, inclusive bounds), `limit` (default 100, cap 10_000), `group_by` ∈ {event_name, agent_id, workspace_id, day} with unsupported fields rejected 400; `/count` and `/daily` (incl. `?days=`) implemented on sqlite, postgres, and mem adapters with matching filter semantics. Handler-level tests (`query_events_group_by_aggregates`, `query_events_since_until_bounds`, `query_events_prefix_match`, `query_events_limit_truncates`) drive the real router.
- **Tests per event (AC5):** every one of the 12 events has at least one per-event test asserting the event and its properties through the production `state.analytics.record` path (mem-backed state, real repository) — no self-confirming or stub-backed tests found; mutation probes above confirm two representative tests kill disabled behavior.

## Hygiene checks

- No check-script weakening: `fabricated-scope-defaults` exemptions shrank 2→1 (gate_executor fix verified in code — emission skipped + warned when MR workspace unresolvable), frozen count lowered accordingly; inert-enforcement change is comment-only.
- Coverage matrix updated honestly: analytics.md rows 1–5 → implemented with landed evidence; rows 6–12 (Decision API) remain task-assigned to task-147, not silently closed.
- Task attribution: all 7 frontmatter commits are ancestors of the candidate; attribution gate exit 0; the trailing process commits are docs-only.
- `tenant_id: "default"` in the sqlite analytics insert is the pre-existing storage-tenant pattern (present verbatim at base), not a new fabricated scope.

## Minor observations (non-blocking, no action required)

- `GET /analytics/events` treats a missing `until` as unbounded rather than defaulting to "now" (spec §Query Parameters). Behaviorally indistinguishable in production since all event timestamps are server-stamped at record time; future-dated rows exist only via direct repository writes (tests).
- Worker-tier `agent.spawned` carries the literal persona `default-worker`, matching agent-runtime.md §Bootstrap (workers run the default-worker persona); orchestrator tiers report their own persona. Spec only requires the property.
