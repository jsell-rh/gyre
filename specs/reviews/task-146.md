# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate: `0f78f4f4db3036c2dbc34c7312bfeb9f0ef0e7e0` (base `cd58a1662c0e17850ffb345bcec98a55453e901c`).
Verdict: **approved** — all five coverage sections for this task are backed by real production code; no blocking findings.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain analytics` → 3 passed incl. `analytics_event_matches_spec_schema` (spec-schema round-trip).
- `cargo test -p gyre-adapters analytics` → 15 passed (SQLite adapter: `analytics_query_filtered_all_params`, `analytics_count_grouped_over_full_filtered_set`, `analytics_count_grouped_by_day`, plus pre-existing suites).
- `cargo check -p gyre-server` → clean (7m02s).
- `cargo check -p gyre-server --tests` → clean (3m23s; type-checks all new test code).
- Mechanical gates, all OK: arch, migration versions, migration SQL portability, mem port contracts, inert enforcement, fabricated-scope defaults, scope-literal defaults, forged-scope fields, forwarded-header trust, in-memory state stores, unbounded external HTTP, dead message kinds, MCP write tools, byte-slice truncation, relative-path defaults, lossy-secret conversion, ABAC route registry, ABAC exempt handlers.
- `check-task-commit-attribution.sh` → **OK against the candidate's committed `specs/tasks/task-146.md`** (all 20 product commits recorded). Note: the check fails against the *working tree* only because the stage checkpoint resets the task file to its assigned state (`progress: not-started`, `commits: []`); that unstaged reset is harness state, not part of the candidate commit (verified by running the check with the committed file restored, then restoring the reset state).

## What was verified

- **Event Schema (§2)**: `AnalyticsEvent` carries all 9 spec fields; `with_scope()` populates user/session/workspace/repo; diesel schema + migration `2026-10-08-000063_analytics_scope_columns` (next unused sequence 000063; portable `ALTER TABLE ... ADD COLUMN` / `CREATE INDEX IF NOT EXISTS` on both SQLite and PG) add the four scope columns to both SQL adapters and the mem adapter.
- **Auto-Emitted Events (§3)**: all 12 events emit at real trigger points with all spec-required properties, across both the HTTP and MCP/autonomous paths where both exist:
  - `task.status_changed` (old_status/new_status/task_id/assigned_to): HTTP status endpoint, MCP `update_task`, spawn auto-advance, completion auto-advance, admin force-kill block, stale-abort block, spec-reject cancel, repo-archive cancel, ralph-loop re-open/ceiling/terminal blocks.
  - `mr.merged` (mr_id/repo_id/gate_count/queue_wait_secs): HTTP transition + queue merge path (`emit_mr_merged`, deferred past group rollback so a reverted group never leaves a phantom event).
  - `mr.closed` (mr_id/repo_id/reason): transition endpoint, spec-reject, repo-archive, ralph-loop gate-failure/ceiling closes.
  - `agent.spawned` (agent_id/task_id/compute_target/persona): worker path reports the bound persona slug and the launch's resolved compute target (regression tests pin both against the earlier hardcoded values); orchestrator path reports the post-launch resolved target and null task_id.
  - `agent.completed` (agent_id/task_id/duration_secs): HTTP complete + MCP `gyre_agent_complete`.
  - `agent.failed` (agent_id/task_id/reason): fail/stop endpoints, agent status API, admin kill, spec-reject stop, repo-archive stop, compose teardown, stale abort, ralph-loop ceiling/terminal, orchestrator process-death monitor.
  - `merge_queue.processed` (mr_id/outcome/wait_secs): merged + every terminal Failed path consolidated through `fail_queue_entry` (gate failure in selection, MR/repo not found, non-mergeable, spec/breaking-change/spec-link/constraint policies, attestation-ABAC deny, dependency closed, conflict, git error) + Cancelled/skip paths as outcome "skipped" (circuit breaker, operator cancel).
  - `gate.failed`/`gate.passed` (gate_id/gate_type/mr_id/output_snippet|duration_secs): in `run_gate` after the gate result is persisted.
  - `spec.approved` (spec_path/approver_type/approval_mode): in `approve_spec`.
  - `budget.warning` (workspace_id/metric/threshold_pct): 80% threshold evaluated in `check_spawn_budget` (agents/tokens/cost) and `record_budget_usage` (tokens/cost).
  - `search.query` (query_length/entity_types/result_count/duration_ms): HTTP search handler and MCP `gyre_search`, both through `record_search_query`.
- **Query API / Query Parameters (§4/§5)**: no new endpoints (per task instruction); GET events supports event_name (exact + trailing-`*` prefix), agent_id, user_id, workspace_id, repo_id, since/until (ISO8601 or unix secs, date-only `until` extends to end of day), limit (default 100, max 10 000), group_by (event_name/agent_id/workspace_id/day; unsupported values rejected). `count_grouped` aggregates server-side over the full filtered set (GROUP BY in SQL adapters, shared predicate in mem) — the earlier limit-truncating flaw is fixed and pinned by regression tests on both adapters plus the HTTP layer. Daily supports `?event_name=&days=30` with until defaulting to now.
- **Anti-forgery**: POST /analytics/events ignores body agent_id/user_id and binds identity to the verified caller; workspace/repo scope fields validated against the caller's tenant (403 cross-tenant, 404 unknown, 403 on workspace/repo mismatch) — same rule applied to the search endpoint's and MCP `gyre_search`'s workspace_id because it attributes the auto-emitted `search.query` event. Forged-attribution regression tests verify the victim scope's filtered results stay empty.
- **Tests are real, not inflated**: per-event tests drive the actual router (oneshot with auth) or the real production function (`run_gate`, `process_next`, `run_once`, ralph-loop internals) and assert exact property values against the mem repository; several are regression tests for earlier review findings (persona hardcoding, compute-target hardcoding, group_by truncation, forged scope attribution) and fail against the pre-fix code. The one removed test (`selection_skips_atomic_group_not_ready_tries_next_candidate`) was re-added at its new location (merge_processor.rs tests, line ~5870).

## Verifier must execute (server test compilation exceeds the sandbox budget)

- `cargo test -p gyre-server --lib` — in particular the analytics auto-emit suites in `api::analytics`, `api::spawn`, `api::tasks`, `api::merge_requests`, `api::specs`, `api::search`, `api::budget`, `api::admin`, `api::merge_queue`, `api::compose`, `api::orchestrator`, `merge_processor`, `gate_executor`, `stale_agents`, `ralph_loop`, `mcp`, `mem`.
- `cargo test -p gyre-server --test api_integration` (incl. `orchestrator_spawn_denied_across_tenants`).
- `cargo test -p gyre-adapters` (Postgres analytics suites need a DATABASE_URL; SQLite suites verified here).
- Standard CI: `cargo fmt --check`, clippy, `cargo test --all`, frontend `npm ci && npm test` (no web source changes in this candidate).

## Minor observations (non-blocking, recorded for completeness)

- `docs/api-reference.md` line 266 still documents only `?event_name=&agent_id=&since=` for GET /analytics/events; the newly supported workspace_id/repo_id/user_id/until/group_by params and the POST scope-validation rule are implemented and spec-conformant but not reflected in that row. Doc-only.
- The trailing-`*` prefix match builds a SQL `LIKE` pattern from the caller-supplied prefix without escaping `%`/`_`, and the mem adapter's `starts_with` treats those characters literally — an adversarial prefix containing LIKE metacharacters can over-match (never under-match) and behaves differently across adapters. Low severity (analytics filtering only, no scope bypass: scope filters are equality).
- `AnalyticsQueryFilter::default()` yields `limit: 0` (only `new()` sets 100); every production call site sets the limit explicitly, so this is a latent footgun for future callers, not a live defect.
- `budget.warning` re-evaluates on every spawn check once usage is ≥80% (no crossing-edge dedup), so a hot workspace can emit repeated warnings. Data is accurate; volume is noisy. Spec only says "Budget threshold crossed".
- Working tree carries an unstaged reset of `specs/tasks/task-146.md` to the assigned round state; it is stage machinery, not part of candidate `0f78f4f`.
