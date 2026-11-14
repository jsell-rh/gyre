# Review — task-149 (Product Analytics §Analytics Dashboard (UI) + §Data Retention)

Spec: `specs/system/analytics.md` §Analytics Dashboard (UI) and §Data Retention.
Candidate: `b5322cb6d64dbbaaf88ec7a52f1e9d05d654078f` (base `3f7871b6052137050937744605ae3dca245e5a64`).
Task-labeled commits: `f2d6db71` (implementation), `af587a17` (repair: daily endpoint total-volume form), `cc026cc6` (repair: test-module imports + clippy). All three listed in the candidate's `specs/tasks/task-149.md` frontmatter.
Verdict: **approved**.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `cd web && npm ci && npx vitest run src/__tests__/AnalyticsView.test.js` → **13/13 passed** (analytics-view-vitest.log).
- Mutation probe 1: broke the component's funnel-rate computation (`c / prev` → `* 2`) → the `computes funnel drop-off rates` test **fails** (restored; working tree clean). The component tests are behavior-asserting, not self-confirming.
- `cargo test -p gyre-adapters --lib sqlite::analytics` → **17/17 passed**, including the new `analytics_aggregate_by_day_no_event_name_totals_across_names`.
- Mutation probe 2: poisoned the SQLite adapter's `None`-event_name branch (`WHERE event_name = 'x-mutation'`) → the new total-volume adapter test **fails** (restored; working tree clean). The `Option<&str>` port-contract change is genuinely exercised in SQL, not just in the mem adapter.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean (the E0425/E0433 test-compile failures from the prior verification round are repaired by `cc026cc6`).
- `cargo clippy -p gyre-server --tests` → no warnings on any changed file (`api/analytics.rs`, `api/admin.rs`, `retention.rs` changes); the only hit near the diff is pre-existing `retention.rs:524` outside changed lines. The CI `check-clippy-diff.py` gate should pass.
- Mechanical invariants: `check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`, `check-migration-sql-portability.sh`, `check-unbounded-external-http.sh`, `check-in-memory-state-stores.sh` → all OK.
- `check-task-commit-attribution.sh` FAILs **only against the working tree**, whose `specs/tasks/task-149.md` was reset by the assignment controller (progress `not-started`, `commits: []`). The committed candidate frontmatter lists all three task-labeled commits — artifact of the assignment process, not a candidate defect.

## Verified working (no findings)

- **AnalyticsView.svelte** (web/src/components/, ~1060 lines) with all five specced sections wired to the real API: SVG 30-day bar chart (zero-filled UTC calendar days, one bar per day, `GET /analytics/daily?days=30`); paginated events table (event/agent/workspace filters from live `group_by`/`/top` data, click-to-expand full properties JSON, timestamp-cursor "Load more" with client-side id dedupe); decision evaluator for exactly the six built-in decisions with client-side JSON validation before the `GET /analytics/decide` call and outcome/confidence/reason/metrics rendering; funnel view with the default spawn → InProgress → complete → merge funnel, per-step drop-off percentages, and validated custom funnel definitions; top-agents table (`GET /analytics/top?dimension=agent`, workspace-scoped).
- **Backend support is real, not stubbed**: `/top?dimension=agent` aggregates per-agent over `query_filtered` (event count, completion rate = completed/(completed+failed) with null-on-empty, avg duration from `duration_secs`); `workspace_id` is tenant-validated through the same `validate_workspace_scope` used by `decide`/`record_event` (403 cross-tenant, 404 unknown — handler test covers both). `/count?property=key:value` filters on property equality with a 400 on malformed filters. `/daily` now accepts absent/empty `event_name` and aggregates across ALL event names — the port contract change (`AnalyticsRepository::aggregate_by_day(Option<&str>)`) is implemented consistently in mem, SQLite, Postgres, and the search_sync wrapper, with the SQLite total-form mutation-verified.
- **Data Retention**: the nightly cleanup job, `analytics_events` 365-day default, and hard-DELETE via `delete_older_than` were already real from task-207 (verified then); task-149 adds the analytics-scoped `PUT /api/v1/admin/retention` body form `{"retention_days": N}` (updates only the `analytics_events` policy in place so the 7-type completeness invariant holds, `>= 1` validated with 0 rejected, sibling policies untouched, durable persist before the 204; invalid forms 400 with policies unchanged — handler tests cover both directions). Route is in the ABAC registry (admin resource) — unchanged from base. Default-365 is pinned in three places (GET endpoint test, `default_policies_present`, cleanup test).
- **Prior verification findings repaired**: the daily-endpoint 400 on bare `?days=30` (fix `af587a17`, with a dedicated regression test `daily_events_bare_days_form_returns_total_volume`) and the test-module compile errors + clippy `unnecessary_sort_by` (fix `cc026cc6`, confirmed via `cargo check --tests` and clippy here).
- **Navigation**: topbar chart button, `/workspaces/:slug/analytics` route with pushState/popState handling, mobile drawer link, Esc-back. The spec's "sidebar: Analytics under Operations" is superseded by the no-sidebar navigation model (ui-navigation.md Principle 5) — documented in the coverage notes; consistent with how every other view navigates.
- Coverage matrix updates (analytics.md 12/12) match shipped behavior; the unrelated SUMMARY row changes (abac-policy-engine, agent-gates) come from merging already-approved task branches (task-124/task-148), not from task-149.

Minor (non-blocking, recorded for completeness):

- `/count?property=` and both `/top` dimensions fetch with a 100,000-event cap (documented in code, pre-existing `/top` pattern); beyond that the counts silently cap. Same class as the pre-existing endpoint, not a regression.
- Frontend "Load more" stops if a page returns exactly PAGE_SIZE events that are all already-seen ids (same-timestamp duplicates) — an edge case that can end pagination early, never duplicates rows.
- `retention_days` accepts any u64 ≥ 1 (spec says u32; no upper bound) — a longer-than-forever window is harmless.

## Verifier must run (sandbox 4 GiB wrapper rejects gyre-server test builds)

- `cargo test -p gyre-server --lib` — in particular: `api::analytics::tests::{daily_events_bare_days_form_returns_total_volume, top_agents_dimension_ranks_and_computes_rates, top_agents_workspace_scope_validated_against_caller_tenant, top_dimension_rejects_unknown, count_property_filter_matches_property_equality}`, `api::admin::tests::{admin_update_retention_analytics_form_updates_analytics_only, admin_update_retention_rejects_invalid_analytics_form}`, `retention::tests::{default_policies_present, run_cleanup_purges_old_and_keeps_new}`.
- `cargo test -p gyre-adapters --lib` (analytics + retention rows).
- `cd web && npm ci && npx vitest run` (full frontend suite).
- CI gates: `scripts/check-clippy-diff.py` and the pre-commit check set.
