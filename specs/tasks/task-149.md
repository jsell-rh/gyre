---
title: "Implement analytics dashboard UI and data retention policy"
spec_ref: "analytics.md §Analytics Dashboard (UI)"
depends_on:
  - task-147
progress: ready-for-review
coverage_sections:
  - "analytics.md §Analytics Dashboard (UI)"
  - "analytics.md §Data Retention"
commits: ["f2d6db71f21c89feb052220e030f5faa754be5e7", "af587a17fcee2909f749dbe050f409d710d8b6db"]
---

## Spec Excerpt

From `analytics.md` §Analytics Dashboard (UI):

The Analytics view in the Svelte dashboard (sidebar: "Analytics" under Operations):

- **Event counts bar chart** — daily event volume for the last 30 days
- **Recent events list** — paginated, filterable by event name, agent, workspace
- **Decision panel** — interactive decision evaluator (run any built-in decision against live data)
- **Funnel view** — define a funnel (spawn → implement → complete → merge) and see drop-off rates
- **Top agents** — ranked by events generated, task completion rate, average duration

From §Data Retention:

Analytics events are retained for 365 days by default (configurable via `PUT /api/v1/admin/retention`). After retention expiry, events are deleted (not archived — analytics is not an audit trail; use the audit system for compliance).

## Implementation Plan

### Analytics Dashboard

1. **Create `AnalyticsView.svelte`** in `web/src/lib/components/`:
   - Page-level component rendered when navigating to the Analytics view

2. **Event counts bar chart:**
   - Fetch from `GET /api/v1/analytics/daily?days=30`
   - Render as a vertical bar chart (use simple SVG bars or a lightweight chart library already in the project)
   - Each bar = one day, height = total event count

3. **Recent events list:**
   - Fetch from `GET /api/v1/analytics/events?limit=50`
   - Paginated table: event_name, agent_id, timestamp, properties preview
   - Filters: event name dropdown, agent selector, workspace selector
   - Click event to expand full properties JSON

4. **Decision panel:**
   - Dropdown to select a built-in decision (feature.promote, etc.)
   - JSON editor for context parameters
   - "Evaluate" button → calls `GET /api/v1/analytics/decide`
   - Display result: outcome, confidence, reason, metrics

5. **Funnel view:**
   - Hardcoded default funnel: agent.spawned → task.status_changed(InProgress) → agent.completed → mr.merged
   - Fetch event counts for each step in the funnel
   - Display as horizontal funnel with drop-off percentages
   - Allow custom funnel definition (list of event names)

6. **Top agents:**
   - Fetch from `GET /api/v1/analytics/top?limit=10&since=<30d-ago>`
   - Table: agent_id/name, event count, completion rate, avg duration
   - Completion rate = agent.completed / (agent.completed + agent.failed)

7. **Navigation integration:**
   - Add "Analytics" to the appropriate navigation section
   - Route: `/analytics` or workspace-scoped equivalent

### Data Retention

8. **Add `PUT /api/v1/admin/retention` endpoint** in `gyre-server`:
   - Request body: `{ retention_days: u32 }` (default: 365)
   - Stores retention config in a settings/config table
   - Admin-only access

9. **Implement retention cleanup job:**
   - Background job (like the existing budget reset job)
   - Runs daily
   - Deletes analytics events older than `retention_days`
   - Logs count of deleted events

10. **Tests:**
    - Component test: AnalyticsView renders with mock data
    - Integration test: retention endpoint updates config
    - Integration test: cleanup job deletes expired events, preserves recent ones
    - Unit test: default retention is 365 days

## Acceptance Criteria

- [ ] AnalyticsView component with all 5 sections (bar chart, events list, decision panel, funnel, top agents)
- [ ] Events list is paginated and filterable
- [ ] Decision panel can evaluate any built-in decision interactively
- [ ] Funnel view shows drop-off rates
- [ ] `PUT /api/v1/admin/retention` endpoint (admin-only)
- [ ] Daily retention cleanup job deletes expired events
- [ ] Default retention: 365 days
- [ ] Analytics accessible via navigation
- [ ] Tests pass

## Agent Instructions

- Read `web/src/lib/components/` for existing Svelte component patterns (Svelte 5 with `$state` runes)
- Read `web/src/routes/` for route/page structure
- Read `docs/ui.md` for UI conventions and navigation model
- Read `crates/gyre-server/src/api/mod.rs` for route registration
- Read `crates/gyre-server/src/background_jobs.rs` (or equivalent) for background job patterns
- For charts, check if a chart library is already in `web/package.json` — if not, use simple SVG
- The Svelte SPA uses fetch API calls to the backend — follow existing API call patterns
- Svelte 5 caveat: `$state` proxies cannot be structured-cloned — use `$state.snapshot()` if needed for workers

## Shipped

The product analytics dashboard (analytics.md §Analytics Dashboard (UI)) and
the analytics-scoped retention configuration surface (§Data Retention).

### Behavior

- **`web/src/components/AnalyticsView.svelte`** — full-page dashboard with
  all five specced sections, wired to the real analytics API:
  - **Event counts bar chart** — `GET /analytics/daily?days=30` rendered as
    an SVG bar chart, one bar per day, zero-filled across all 30 calendar
    days (the chart shows volume over time, not just active days), peak
    count in the axis meta. The endpoint returns TOTAL daily event volume
    across all event names (review repair af587a17): `event_name` is an
    optional filter on `GET /analytics/daily` — absent or empty (the
    spec's `?event_name=&days=30` form) aggregates across every event
    name via `AnalyticsRepository::aggregate_by_day(Option<&str>)`, so
    the bare `?days=30` request the chart sends is valid and returns the
    aggregate series, not a 400 and not a single-name series.
  - **Recent events** — paginated table (event, agent, timestamp, properties
    preview) from `GET /analytics/events?limit=50`; filterable by event
    name, agent, and workspace (dropdown options derived from the live
    data via `/top` and `group_by` queries); clicking a row expands the
    full properties JSON; "Load more" advances a timestamp cursor
    (oldest seen timestamp) with client-side id dedupe so same-second
    events are never duplicated across pages.
  - **Decision panel** — dropdown of exactly the six built-in decisions,
    editable JSON context, Evaluate → `GET /analytics/decide` (context
    JSON-validated client-side; invalid JSON never reaches the API);
    result rendered with outcome fields (promote/rollback/healthy,
    confidence, reason) highlighted and metrics below.
  - **Funnel view** — default funnel agent.spawned →
    task.status_changed(new_status:InProgress) → agent.completed →
    mr.merged with per-step counts from `GET /analytics/count` (the
    InProgress step uses the new `property=key:value` filter) and
    drop-off percentages between steps; custom funnel definitions
    (one `event_name [key:value]` per line, validated with inline
    errors, minimum 2 steps) replace and refetch the funnel.
  - **Top agents** — `GET /analytics/top?dimension=agent&limit=10&since=<30d>`
    table: rank, agent, event count, completed, failed, completion rate
    (— when the agent neither completed nor failed), average completion
    duration; scoped to the current workspace.
  - Retention window surfaced read-only from `GET /admin/retention`
    ("Events retained N days"; hidden for callers who cannot read it).
- **Backend support** (real queries, no new routes — all ABAC-registered
  already):
  - `GET /analytics/top?dimension=agent` ranks agents by events generated
    with completion rate (`agent.completed / (completed + failed)`, null
    when neither) and mean `duration_secs` over `agent.completed`; the
    optional `workspace_id` filter is validated against the caller's
    tenant (403 cross-tenant, 404 unknown — same rule as `decide`),
    unknown dimensions are rejected 400.
  - `GET /analytics/count` gained the optional `property=key:value`
    equality filter for funnel steps and optional `since`/`until`
    (defaults 0/now); malformed property filters are rejected 400.
  - `PUT /admin/retention` accepts the analytics.md §Data Retention body
    form `{"retention_days": N}` in addition to the complete 7-type
    policy list: it updates only the `analytics_events` policy (>= 1
    validated; 0 would purge everything on the next nightly run), leaves
    every sibling policy untouched, and persists durably before the 204.
    The enforcement itself was already real (business-continuity §5,
    task-207): 365-day default for analytics events, nightly 02:00 UTC
    cleanup job hard-deleting expired events via `delete_older_than` on
    the SQLite/Postgres/mem adapters with per-run purge counts logged.
- **Navigation** (ui-navigation.md two-mode model — the spec's sidebar no
  longer exists, Principle 5): top-bar chart button next to the workspace
  settings gear, `/workspaces/:slug/analytics` URL route (parse/urlFor,
  popstate and initial-load handling), sub-page breadcrumb with back
  arrow, mobile drawer link, Esc returns to workspace home. `api.js`
  helpers: `analyticsDaily`, `analyticsCount`, `analyticsDecide`,
  `adminRetention`, `setAnalyticsRetention`.

### Test evidence

- Review repair (af587a17) probes, run in this sandbox against the real
  adapters (not mocks):
  - `cargo check -p gyre-server -p gyre-adapters` — clean (exit 0).
  - `cargo test -p gyre-adapters --lib sqlite::analytics::tests::analytics_aggregate_by_day`
    — 2 tests pass, including the new
    `analytics_aggregate_by_day_no_event_name_totals_across_names`
    (seeds two distinct event names on one day; `None` must count both).
  - New gyre-server test
    `daily_events_bare_days_form_returns_total_volume` pins the exact
    dashboard request: bare `?days=30` → 200 with totals across distinct
    event names (would fail with 400 under the old required-param
    contract and fail if the response were per-name), empty-value
    `?event_name=&days=30` → same totals, explicit name still filters.
    **gyre-server test compilation exceeds the in-sandbox 4 GiB budget —
    the independent verifier must run:**
    `cargo test -p gyre-server --lib analytics::tests` (covers the new
    daily-form test plus all analytics handler, decide, top-agents, and
    retention suites).
- `web/src/__tests__/AnalyticsView.test.js` — 13 component tests, all
  passing: five-section render from real API mocks, 30 zero-filled bars,
  properties-JSON expansion, event-name filter reaching the API, cursor
  pagination (`until` = oldest timestamp), decision evaluation with
  edited context (and invalid-JSON rejection without an API call),
  funnel drop-off math (100→50→25→10), custom funnel apply + malformed
  rejection, top-agent rates/durations, retention display. Full web
  suite: 114 files / 2321 tests pass.
- Rust tests added (gyre-server): `top_agents_dimension_ranks_and_computes_rates`,
  `top_agents_workspace_scope_validated_against_caller_tenant`,
  `top_dimension_rejects_unknown`, `count_property_filter_matches_property_equality`,
  `admin_update_retention_analytics_form_updates_analytics_only`,
  `admin_update_retention_rejects_invalid_analytics_form`, plus the
  default-365 assertions strengthened in `admin_list_retention_returns_defaults`
  and `default_policies_present`. **Not executed in this sandbox** — no
  crates.io access and no vendored registry, so no Rust target can build
  here. The independent verifier must run:
  `cargo test -p gyre-server analytics` and `cargo test -p gyre-server retention`
  (plus `cargo check -p gyre-server` / clippy, which are owned by
  verification anyway). Retention cleanup job behavior (deletes expired,
  preserves recent, nightly schedule) is covered by the pre-existing
  task-207 test suite (`run_cleanup_purges_old_and_keeps_new`, which
  asserts the analytics-events purge direction).

### Sandbox limitations recorded

- Rust compilation impossible offline (no registry cache) — see above;
  server-side tests are enumerated for the verifier, not skipped
  silently.
- Mechanical shell checks run clean on the changed files: arch, api-auth,
  abac-route-registry, forged-scope-fields, inert-enforcement,
  relative-path-defaults, fabricated/scope-literal-defaults,
  lossy-secret-conversion, unbounded-external-http, unwired-port-methods.
  `check-dead-components` reports 4 violations, all pre-existing at the
  base commit (CanvasProbeStub, PipelineOverview, __accordion_*).
- No browser/listener probes were needed (component tests run in jsdom).
