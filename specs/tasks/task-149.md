---
title: "Implement analytics dashboard UI and data retention policy"
spec_ref: "analytics.md §Analytics Dashboard (UI)"
depends_on:
  - task-147
progress: not-started
coverage_sections:
  - "analytics.md §Analytics Dashboard (UI)"
  - "analytics.md §Data Retention"
commits: []
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
