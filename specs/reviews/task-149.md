# Review — task-149 (Implement analytics dashboard UI and data retention policy)

Spec: `specs/system/analytics.md` §Analytics Dashboard (UI) + §Data Retention.
Candidate: `b248e5c1576c3d2446732979377c7e9822fae8fa` on
`pipeline/task-149/f2b12672ee1a475fb82709c08129e3cd-1`, diff against assigned base
`44cf25bf0bea544979fcae88c4d80b1165964fbe`. The candidate is the approved
implementation line merged with main @ base (merge `32882eff`); Rust/Svelte source
auto-merged cleanly, the only textual conflicts were generated `web/dist` assets
(resolved to main's; verifier rebuilds), plus a real repair: task-149's duplicate
`adminRetention` key in `web/src/lib/api.js` was dropped in favor of the pre-existing
binding. Verified `api.js` now has exactly one `adminRetention` (line 340).
Verdict: **approved**.

## Evidence (review sandbox; full logs in /tmp/stage/review-evidence)

- `web`: `npm ci` then `npx vitest run src/__tests__/AnalyticsView.test.js` →
  13/13 pass; full `npm test` → 116 files, 2364 passed / 41 skipped, exit 0.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → exit 0
  (new server test code type-checks).
- `cargo test -p gyre-adapters --lib analytics` → 17/17 pass, including the new
  `analytics_aggregate_by_day_no_event_name_totals_across_names` (cross-name totals).
- Invariant scripts all exit 0: check-arch, check-abac-route-registry,
  check-mem-port-contracts, check-inert-enforcement, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-unbounded-external-http,
  check-forwarded-header-trust.

## Verified working

- **AnalyticsView.svelte** (web/src/components/) implements all five specced sections
  against the real API with matching response shapes: SVG 30-day bar chart zero-filled
  over UTC calendar days from `GET /analytics/daily?days=30` (`{date,count}` keys
  match the server's `DayCount`); paginated recent-events table (50/page,
  timestamp-cursor "Load more" with id dedupe, event/agent/workspace filters whose
  option lists come from the live `group_by` object responses); decision evaluator for
  exactly the six built-ins with client-side JSON validation before the call; funnel
  with per-step drop-off percentages and validated custom funnel definitions
  (`event_name [key:value]`, ≥2 steps); top-agents table (rank, events, completed,
  failed, completion rate, avg duration).
- **Navigation**: topbar chart button (`data-testid="ws-analytics-btn"`),
  `/workspaces/:slug/analytics` route with parsePath/pushState/popstate/refresh
  restore, mobile drawer link, Esc back — consistent with ui-navigation.md
  Principle 5 and the WorkspaceSettings precedent.
- **Backend**: `GET /analytics/top?dimension=agent` aggregates per-agent
  (events, completed/failed, completion_rate = completed/(completed+failed),
  avg duration_secs) through the analytics port with `workspace_id` tenant-validated
  (403 cross-tenant, 404 unknown — asserted by tests through `build_router` with full
  middleware); unknown dimension 400. `GET /analytics/count` gains optional
  since/until and a `property=key:value` string-equality filter (malformed 400).
  `GET /analytics/daily` treats absent/empty `event_name` as total volume across all
  names; the port signature `aggregate_by_day(Option<&str>)` is propagated to every
  implementor (SQLite, Postgres, mem, search_sync delegate) with a new SQLite
  cross-name totals test.
- **Data Retention**: `PUT /api/v1/admin/retention` accepts the analytics-scoped
  `{ "retention_days": N }` form (N ≥ 1 validated; 0/string/other-object forms 400
  with policies unchanged — tested), updates only the `analytics_events` policy from
  the store's own list, persists durably before the 204. Default 365 asserted both in
  the endpoint GET test and `default_policies_present`. Nightly cleanup
  (delete_older_than on analytics_events, purge counts logged) was already real from
  task-207; this task's additions are pinned by tests. Route is pre-existing and
  ABAC-registered (`abac_middleware.rs:466`).
- **Tests are hard, not inflated**: the funnel test asserts computed drop-off text
  from seeded counts (100→50→25→10 ⇒ "50% continue (50% drop-off)" etc.) and that the
  InProgress step's request carries `property=new_status:InProgress`; the daily test
  seeds two distinct event names on one day and asserts total=3 vs per-name=2 (would
  fail if the endpoint 400'd on the bare form or returned a single-name series); the
  pagination test asserts the cursor value (`until` = oldest page timestamp); invalid
  decision JSON never reaches the API.

## Deferred to the verifier (build wrapper blocks gyre-server test execution in review)

`cargo test -p gyre-server api::analytics`, `... api::admin`, `... retention`;
workspace-wide `cargo test --all` + Clippy + web build; and
`scripts/check-task-commit-attribution.sh` (timed out here — needs full git history;
frontmatter lists the four product commits f2d6db71, af587a17, cc026cc6, 7e0fbc2a,
matching the branch's product-surface commits).

## Observations (non-blocking)

- Canonical `analytics.md` still says "sidebar: Analytics under Operations"; the app
  has no sidebar by ui-navigation.md Principle 5 ("No sidebar"), and the
  implementation follows the newer nav spec (topbar button + route + drawer), the
  same precedent WorkspaceSettings set. Documented in the coverage row. A future spec
  lifecycle pass could align the analytics.md wording.
- `GET /analytics/events`, `/daily`, and `/count` remain tenant-blind aggregations
  (pre-existing design from task-146/147, untouched by this diff); the dashboard's
  daily chart and funnel therefore span all workspaces, while top-agents is
  workspace-scoped. Not a regression introduced by this candidate.
- During my probe, build.rs regenerated `web/dist` with new hashes; restored to HEAD —
  generated output is verifier-owned and rebuilt remotely.
- `specs/tasks/task-149.md` in the working tree was pre-modified by the stage (frontmatter
  reset to the assignment's `not-started`/`commits: []` body); the committed version at
  the candidate records `ready-for-review` with the four product commits. Left untouched.
