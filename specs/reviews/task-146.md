# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters.
Assignment base `4a203a066`, candidate `f235da673` (branch `pipeline/task-146/6a31aec0834e4a6d851ad7b901a9859e-2`).
Verdict: **approved** — full evidence in `/tmp/stage/review-evidence/review-summary.md`.

## What was verified

**Event schema (AC 1).** `gyre-domain/src/analytics.rs` `AnalyticsEvent` carries all 9
spec fields; `with_scope` populates user/session/workspace/repo. Id-typed fields are
`Option<String>`, matching the storage layer's TEXT columns (documented in the coverage
row). The serde round-trip test `analytics_event_matches_spec_schema` pins the shape —
run this round: 3/3 gyre-domain analytics tests pass.

**All 12 auto-emitted events at real trigger points (AC 2, 3).** Every event name was
grepped and each emission site read in the diff:

- `task.status_changed` — one helper (`api/analytics.rs:69`) called from 9 production
  transition paths (HTTP, MCP `gyre_update_task`, spawn-assign, completion auto-advance,
  admin kill, stale-abort, spec-reject cancel, repo-archive cancel); props
  `old_status`/`new_status`/`task_id`/`assigned_to` all populated from real task state.
- `mr.merged` — both merge paths (HTTP transition and queue merge) share the same
  property contract; `gate_count` is a real `gate_results.list_by_mr_id` count,
  `queue_wait_secs` a real enqueue-time lookup (None when never queued, not fabricated).
- `mr.closed` — HTTP close, spec-reject (`reason: "spec_rejected"`), repo-archive
  (`reason: "repo_archived"`).
- `agent.spawned` — worker path (real `launch.compute_target_label`, persona), orchestrator
  tiers, ralph-loop re-spawn.
- `agent.completed` / `agent.failed` — completion + MCP complete; failure/kill/stale-abort/
  compose-teardown/spec-reject/archive paths, each with a real `reason` and measured
  `duration_secs`.
- `merge_queue.processed` — every terminal path routed through `fail_queue_entry` /
  `emit_queue_processed_terminal` so status change and event cannot diverge; outcome
  merged/failed/skipped, `wait_secs` from real enqueue time.
- `gate.failed`/`gate.passed` — emitted in `gate_executor::run_gate` after real execution
  with the actual output snippet / measured duration.
- `spec.approved` — approval handler, props from the recorded approval event + ledger mode.
- `budget.warning` — 80% threshold in the budget-check and usage-recording paths, metrics
  agents/tokens/cost; below-threshold non-emission tested.
- `search.query` — main search path after the real index query; props
  `query_length`/`entity_types`/`result_count`/`duration_ms` asserted with concrete
  values in the test (e.g. `query_length == 14` for `"reconciliation"`).

**Query API completeness (AC 4).** `query_events` supports the full spec parameter set:
`event_name` (exact + trailing-`*` prefix), `agent_id`, `user_id`, `workspace_id`,
`repo_id`, `since`/`until` (ISO8601 or unix seconds), `limit` (default 100, cap 10_000),
`group_by` (event_name/agent_id/workspace_id/day; unsupported fields rejected). All three
adapters implement `query_filtered` (Diesel boxed queries on SQLite/Postgres; mem parity
test `query_filtered_matches_sqlite_filter_semantics`). Run this round: 13/13
gyre-adapters analytics tests pass, including `analytics_query_filtered_all_params`,
which seeds cross-cutting data and asserts each scope filter individually.

**record_event hardening (review-repair round carried in candidate).** Identity is
derived from the verified caller (body `agent_id`/`user_id` ignored; impersonation
attributes to the impersonator), workspace/repo scope validated against the caller's
tenant via the shared audit.rs helpers — cross-tenant attribution is Forbidden, unknown
ids NotFound. Regression tests at analytics.rs:1220/1262/1340.

**Tests are real (AC 5).** Per-event tests at every trigger point assert concrete
property values, so removing an emission or a required property fails them. The one
rewritten merge_processor selection test was moved, not lost (single definition,
original assertions intact). No assertionless/self-confirming tests found in the diff.

**Mechanical gates.** 19 `scripts/check-*.sh` gates pass on the candidate, including
arch, migration-versions, migration-sql-portability (new migration
`2026-10-08-000061_analytics_scope_columns` is unique and dialect-portable),
abac-route-registry, mem-port-contracts, inert-enforcement, fabricated-scope-defaults
(exemption count lowered 2→1 with a fix, no new entries). Attribution gate passes
against the committed candidate (all 12 product-surface commits recorded in frontmatter).
`cargo check -p gyre-server --tests` clean; `cargo fmt --check` clean on all four
packages.

## Sandbox restrictions (not defects) — verifier must run

The build wrapper rejects gyre-server test-code generation under the 4 GiB budget
("Server builds/tests belong to the independent verifier"). The verifier must run the
per-event server tests listed in `/tmp/stage/review-evidence/review-summary.md` (focused
`cargo test -p gyre-server --lib` filters per module, plus
`--test api_integration orchestrator_spawn_denied_across_tenants` and
`cargo clippy -p gyre-server --all-targets`). `scripts/check-tenant-filter.sh` fails only
under the sandbox's mawk 1.3.4; the script is unchanged from base.

## Minor observations (non-blocking)

- `mr.closed` `reason` on the generic HTTP close path is the constant
  `"closed_without_merge"` (the transition request carries no reason field); the other
  close paths carry specific reasons. Spec only requires a `reason` property.
- `spec.approved` is emitted only when the ledger entry exists and `current_sha` matches
  the approved sha — an approval recorded against a missing/stale ledger entry does not
  emit. Defensible: the ledger is the authoritative approval status; no spec-approved
  transition occurs on that path.
- The `event_name` trailing-`*` prefix filter passes the prefix into SQL `LIKE`
  unescaped, so `%`/`_` inside the prefix act as wildcards. Spec only requires the
  `prefix*` form; cosmetic at most.

Generated `web/dist/` untouched by this diff (no frontend changes); nothing to record
for the verifier's bundle stage beyond the standard rebuild.
