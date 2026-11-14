# Review — task-149 (Analytics Dashboard UI + Data Retention)

Spec: `specs/system/analytics.md` §Analytics Dashboard (UI) and §Data Retention.
Candidate: `095fe8a872501a84db458f6c8af9bab4dd868b8e` (branch `pipeline/task-149/7c908d443a934383ae1d6ce3ec81121f-1`, base `9afd66c7`). All six `commits:` frontmatter entries verified present in the candidate's ancestry.
Verdict: **approved** (round 1). Evidence: `/tmp/stage/review-evidence/probe-summary.md`.

## What was verified

### Analytics Dashboard (UI)

- **AnalyticsView.svelte** (`web/src/components/`) implements all five specced sections and is a live component: imported and rendered by `App.svelte` (mode `analytics`), reachable via the top-bar chart button next to the settings gear, the `/workspaces/:slug/analytics` URL route (parse/urlFor, popstate + initial-load handling), the mobile drawer link, and Esc-back — consistent with ui-navigation.md's two-mode model (the spec's sidebar was superseded by Principle 5; this is a spec-lifecycle-sanctioned deviation, documented in the task notes and docs/ui.md).
- **Bar chart**: `GET /analytics/daily?days=30` rendered as SVG bars, zero-filled across all 30 calendar days, `dailyMax` guarded against div-by-zero.
- **Recent events**: paginated table (event/agent/timestamp/properties), filterable by event name, agent, and workspace; row click expands the full properties JSON; "Load more" advances a timestamp cursor with client-side id dedupe.
- **Decision panel**: dropdown of exactly the six built-in decisions, editable JSON context (client-side validation rejects invalid JSON before the API call), Evaluate → `GET /analytics/decide`, result rendered with outcome/confidence/reason highlighted plus metrics.
- **Funnel**: default spawn → InProgress → complete → merge with per-step counts (InProgress via the new `property=new_status:InProgress` filter) and drop-off percentages; custom funnel definitions validated with inline errors (min 2 steps, `key:value` shape).
- **Top agents**: `GET /analytics/top?dimension=agent&limit=10&since=<30d>` table with event count, completed/failed, completion rate (— when neither), avg duration; scoped to the current workspace.
- Mock/API contract cross-checked against the real backend serialization shapes (`AnalyticsEvent` fields, `{date,count}`, `{event_name,count}`, `TopAgentEntry`, retention policy list) — the mock-driven tests exercise the real contract, not an invented one.

### Backend support

- `aggregate_by_day(Option<&str>)`: `None` = total daily volume across all event names — implemented consistently in ALL FOUR adapters (sqlite, postgres, mem, search_sync wrapper) with a both-directions SQLite adapter test (`analytics_aggregate_by_day_no_event_name_totals_across_names`, run here: 17/17 sqlite analytics tests passed). The bare `?days=30` and `?event_name=&days=30` forms map to `None` (trim + empty-filter).
- `GET /analytics/top?dimension=agent`: per-agent aggregation over `query_filtered`, completion rate `agent.completed/(completed+failed)` (null when neither), mean `duration_secs` over completions, unknown dimension 400. The optional `workspace_id` is tenant-validated like `decide` (403 cross-tenant, 404 unknown) — stricter than the pre-existing unscoped event-name dimension, which this diff does not change.
- `GET /analytics/count`: optional `property=key:value` equality filter (malformed → 400), `since`/`until` now optional (0/now defaults), `since > until` rejected.

### Data Retention

- `PUT /api/v1/admin/retention` accepts the analytics.md form `{"retention_days": N}`: rejects 0 / non-u64 / wrong-key bodies with 400 leaving policies untouched; updates only the `analytics_events` policy sourced from the store's current list (siblings cannot be silently reset — this addresses the task-207 F4 class for the new form); persists durably before the 204. The complete 7-type list form and its `validate_policies` gate are unchanged.
- Enforcement is real and pre-existing (task-207/business-continuity §5): 365-day analytics default, nightly 02:00 UTC cleanup hard-deleting expired events via `delete_older_than` on all adapters; this task pinned the default-365 assertions (`default_policies_present`, `admin_list_retention`) and surfaced the configured window read-only in the dashboard (hidden for non-admin callers).
- Route is pre-registered in the ABAC registry (`check-abac-route-registry.sh` clean); admin-only gating unchanged.

## Probes run (smallest meaningful set)

- `npm ci` + `npx vitest run src/__tests__/AnalyticsView.test.js` → **13/13 passed**; full `npx vitest run` → **2364 passed, 0 failed** (App.svelte shell changes covered by existing suites).
- **Mutation checks** (temporary, restored; worktree clean after): breaking the funnel drop-off math, removing the top-agents workspace scope, and disabling the 30-day zero-fill each killed exactly the test covering that behavior — the tests are not self-confirming.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean (new handler tests type-check).
- `cargo test -p gyre-adapters --lib sqlite::analytics::tests` → 17/17 passed.
- Invariant scripts: arch, ABAC route registry, mem port contracts, forged scope fields, api-auth, unwired port methods, inert enforcement, scope-literal/fabricated-scope defaults, stale-stub/placeholder markers, event payload — all clean. The failing scripts (dead-components, stale-enforcement-mode, self-confirming/tautological/mirrored JS) fail identically with the candidate's files removed — pre-existing, not candidate-caused.

## Deferred to the verifier (in-sandbox 4 GiB build wrapper blocks gyre-server test codegen — transport/infrastructure restriction, not a code defect)

- `cargo test -p gyre-server --lib analytics::tests` (incl. `daily_events_bare_days_form_returns_total_volume`, `top_agents_dimension_ranks_and_computes_rates`, `top_agents_workspace_scope_validated_against_caller_tenant`, `top_dimension_rejects_unknown`, `count_property_filter_matches_property_equality`)
- `cargo test -p gyre-server --lib admin::tests` (incl. `admin_update_retention_analytics_form_updates_analytics_only`, `admin_update_retention_rejects_invalid_analytics_form`)
- `cargo test -p gyre-server --lib retention::tests` (incl. `run_cleanup_purges_old_and_keeps_new`, `default_policies_present`)
- `cargo test -p gyre-server --test api_integration -- analytics_record_and_query admin_retention_list_and_update`

## Non-blocking observations

- `api.setAnalyticsRetention` is exported but not used by the UI (retention is displayed read-only). The spec requires configurability via the endpoint, which exists and is tested; not a defect.
- `cargo check` triggers build.rs web rebuild; the regenerated `web/dist` was restored — dist freshness is the verifier's surface, no observation made.
- `GET /analytics/events`' unvalidated `workspace_id` query filter is pre-existing (task-146, unchanged by this diff); the new agent dimension's stricter tenant validation does not regress it.
