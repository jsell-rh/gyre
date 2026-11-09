# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (§Purpose context).

Candidate under review: `a34d06c065446174bf3a5df7a28e47a8cb5604c4`, base `fefde2946f3a0af1f212588f54591516fee0f7b2`.
Cumulative diff: 37 files, +4514/−284. The candidate adds only the rate-limit
determinism fix (`d7641dddc`, middleware.rs + rate_limit.rs) and two process
commits (`51fa03460`, `a34d06c06`) that touch only `specs/tasks/task-146.md`
(frontmatter `commits:` updated to include `d7641dddc`; verification-repair
round notes) on top of the previously reviewed `2c8b65581`.

Verdict: **complete** — approved.

## Acceptance criteria vs. evidence

- **AnalyticsEvent struct matches spec schema** — `gyre-domain/src/analytics.rs`
  carries all 9 spec fields (`id`, `event_name`, `agent_id`, `user_id`,
  `session_id`, `workspace_id`, `repo_id`, `properties`, `timestamp`);
  `with_scope` populates the scope ids; serde round-trip test
  `analytics_event_matches_spec_schema` pins the shape (gyre-domain suite:
  437 passed). The four new id/scope fields use `Option<String>` (not the
  spec excerpt's `Option<Id>`) — a documented representation choice matching
  the pre-existing `agent_id: Option<String>` and the storage layer's TEXT
  columns; all fields are present, named, and typed consistently with the
  rest of the struct. Not a hollow reading of the schema.
- **All 12 auto-emitted events recorded at trigger points** — verified by
  reading every emission site: `task.status_changed`
  (`record_task_status_changed`, called from the HTTP status endpoint, MCP
  `update_task`, spawn/completion auto-advance, admin force-kill, stale
  abort, spec-reject and repo-archive cancellation), `mr.merged` (HTTP
  transition + queue merge via `emit_mr_merged`), `mr.closed` (HTTP
  transition + repo-archive + spec-reject), `agent.spawned` (spawn.rs worker
  + orchestrator.rs tiers), `agent.completed` (spawn.rs HTTP + mcp.rs
  `gyre_agent_complete`), `agent.failed` (fail/admin-kill/stale-abort/
  spec-reject stop/repo-archive stop/compose teardown),
  `merge_queue.processed` (every terminal path — merged, every `Failed`
  route through `fail_queue_entry`/`emit_queue_processed_terminal`,
  circuit-breaker Cancelled and operator cancel as outcome `skipped`),
  `gate.failed`/`gate.passed` (gate_executor.rs after execution),
  `spec.approved` (specs.rs approval handler), `budget.warning`
  (`emit_budget_warning` from `check_spawn_budget` and
  `emit_usage_warnings`), `search.query` (search.rs after execution).
- **Spec-required properties** — each emission site's JSON includes the
  spec property list (checked individually; e.g. `mr.merged` carries
  `gate_count` from real `gate_results.list_by_mr_id` and `queue_wait_secs`
  from real queue-entry `enqueued_at`; `gate.failed` carries
  `output_snippet` via char-boundary-safe truncation; `search.query`
  carries `query_length`/`entity_types`/`result_count`/`duration_ms`).
- **Query API supports all filter parameters** — `QueryEventsParams` has
  `event_name`, `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`,
  `until`, `limit` (default 100, cap 10 000), `group_by`
  (event_name/agent_id/workspace_id/day; unsupported fields rejected);
  `AnalyticsRepository::query_filtered` implements the full set on SQLite
  (LIKE prefix match), Postgres, and mem (adapter test
  `analytics_query_filtered_all_params`; gyre-adapters analytics: 13
  passed).
- **Tests pass for each auto-emitted event** — focused suites all green:
  analytics-filtered suites 42 passed, `merge_processor::tests` 71 passed,
  `api::specs` 81, `api::spawn` 43, `api::merge_requests` 35,
  `api::search` 11, `api::budget` 26, `gate_executor::tests` 26,
  `api::compose` 8, `api::merge_queue` 9, `api::orchestrator` 7,
  `stale_agents`/`mcp` analytics tests included in the filtered runs;
  `api_integration` 67 passed; full `gyre-server --lib`: **1527 passed,
  0 failed**.

## This round's change (d7641dddc): rate-limit test determinism

The prior verification's only failure (`cargo test --all`:
`middleware::tests::rate_limit_exhaustion_returns_429`, left 200 right 429)
was a wall-clock race: `RateLimiter` refills to full rate on each new
wall-clock second, so two back-to-back requests straddling a second
boundary let the second draw a refilled token. The fix injects the clock
(`with_clock`); production `RateLimiter::new` still binds the real
`now_secs` (production behavior unchanged, verified in the diff), the
exhaustion test freezes the clock, and the refill branch gains its first
deterministic tests (`refills_to_full_rate_each_second`,
`frozen_clock_keeps_bucket_exhausted`). Assertion strength is unchanged
(first request 200, second 429).

Probes: `rate_limit`/`middleware` filtered runs 19 passed;
`rate_limit_exhaustion_returns_429` run 10× serially — 10/10 pass; full
`gyre-server --lib` (1527 tests, same scale as the verification failure) —
0 failures including the previously flaky test.

## Mutation probes (test strength, independently run this round)

- Disabled the `state.analytics.record(&ev)` call inside
  `emit_queue_processed_terminal` (merge_processor.rs:2121) →
  `merge_processor::tests`: **6 FAILED** (`non_mergeable_mr_during_selection…`,
  `atomic_group_non_mergeable_member…`, `gate_failure_during_selection…`,
  `mr_not_found_during_selection…`, `spec_policy_block…`,
  `circuit_breaker_cancel…`). Restored; tree verified pristine.
- Renamed the `mr.closed` event to a probe value in the HTTP transition
  emission (merge_requests.rs) → `closing_mr_emits_analytics_event`
  **FAILED**. Restored; tree verified pristine.

Both prove the per-event tests fail when the production emission is
disabled — not self-confirming.

## Exemption-file changes (all reductions, fixes verified real)

- `abac-exempt-handlers-exemptions.txt`: both orchestrator spawn entries
  removed — `spawn_workspace_orchestrator` and `spawn_repo_orchestrator`
  now load the workspace/repo, resolve its tenant, and return
  `Forbidden` on mismatch (orchestrator.rs, verified in source; suite
  passes).
- `inert-enforcement-exemptions.txt`: both `validate_persona` entries
  removed — helper renamed `warn_unapproved_persona` (advisory by name,
  both call sites updated).
- `fabricated-scope-defaults-exemptions.txt` / check script:
  `FROZEN_EXEMPTION_COUNT` 3 → 1 — gate_executor.rs GateFailure ws_id
  fallback fixed for real (emission skipped + `tracing::warn!` when the
  MR's workspace is unresolvable, verified in source);
  `speculative_merge.rs:372` entry remains.

All mechanical gates pass on the candidate: task-commit-attribution,
fabricated-scope, scope-literal, migration-versions,
migration-sql-portability, arch, inert-enforcement, abac-route-registry,
abac-exempt-handlers, mem-port-contracts, mcp-write-tools,
dead-message-kinds, byte-slice-truncation, relative-path,
lossy-secret-conversion, forwarded-header-trust, forged-scope-fields,
in-memory-state-stores, unbounded-external-http — exit 0 each.

## Notes (non-blocking)

- The analytics scope-column migration
  (`2026-10-08-000059_analytics_scope_columns`) uses portable
  `ALTER TABLE … ADD COLUMN` / `CREATE INDEX IF NOT EXISTS` (and `DROP
  COLUMN` in down.sql) — runs on both SQLite and Postgres;
  `check-migration-sql-portability.sh` exit 0.
- SQLite `LIKE` prefix matching is ASCII-case-insensitive by default; the
  spec does not specify case sensitivity for `event_name` prefix filters
  and event names are conventionally lowercase — cosmetic only.
- Coverage matrix (`specs/coverage/system/analytics.md`) marks exactly the
  five task-146 sections implemented; sections 6–12 remain `task-assigned`
  to task-147 (Decision API), correctly untouched here.

Probe evidence: `/tmp/stage/review-evidence/` (suite outputs, repeat runs,
mutation probes, check-script logs).
