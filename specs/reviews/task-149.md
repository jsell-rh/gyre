# Review — task-149 (Analytics Dashboard UI + Data Retention)

Spec: `specs/system/analytics.md` §Analytics Dashboard (UI), §Data Retention.
Candidate: `5e33e3228383e76b66510822bae2f34dd4f41a11` (base `4b9220bc`, product
commit `f2d6db71`). Verdict: **needs-revision** (1 blocking finding).

## What was verified

Frontend (executed in this sandbox):

- `npm ci` then `npx vitest run src/__tests__/AnalyticsView.test.js` → 13/13
  pass. Full suite `npx vitest run` → 114 files / 2321 passed, 41 skipped.
- Mutation probes (each reverted; tree restored to the exact candidate —
  `diff` against a saved copy confirmed clean):
  - remove bar-chart zero-fill → 1 test fails (zero-fill test kills it);
  - drop the pagination `until` cursor → 1 test fails;
  - drop the funnel step-2 `new_status:InProgress` property filter → 1 test
    fails.
  The events/funnel/top-agents/decision tests are genuinely behavior-sensitive.
- Mechanical checks on the changed tree: arch, abac-route-registry,
  api-auth, forged-scope-fields, inert-enforcement, scope-literal-defaults,
  fabricated-scope-defaults, conditional-test-guards → all clean.
  `check-dead-components` reports 4 violations, byte-identical with the base
  commit (pre-existing). `check-task-commit-attribution` failure is the
  pre-existing task-095 entry (5aaded21, present at base).

Backend (manual review only — this sandbox has no crates.io access, so no
Rust target can build; `curl https://index.crates.io` and
`https://static.crates.io` both fail with connection errors — infrastructure
restriction, not a code defect):

- `GET /analytics/top?dimension=agent` — real per-agent aggregation through
  `query_filtered` (not a stub); completion rate `completed/(completed+failed)`
  with null when neither; avg `duration_secs` over `agent.completed`;
  unattributed events excluded; `workspace_id` validated against the caller's
  tenant via the shared `audit::validate_workspace_scope` (403 cross-tenant,
  404 unknown — same rule as `record_event`/`decide`), tested in
  `top_agents_workspace_scope_validated_against_caller_tenant`.
- `GET /analytics/count` — optional `since`/`until` (back-compatible;
  previously required) and `property=key:value` string-equality filter for
  funnel steps, malformed filter → 400.
- `PUT /admin/retention` — the analytics form `{retention_days: N}` updates
  only the `analytics_events` policy (N ≥ 1 validated; 0/negative/types
  rejected 400 with policies untouched — tested). Route is ABAC-registered
  under the `admin` resource (admin-only via builtin-admin-only-deny). The
  nightly 02:00 UTC cleanup job and the 365-day default are pre-existing,
  real enforcement from task-207 (`run_cleanup` →
  `state.analytics.delete_older_than(cutoff)`); the candidate adds the
  spec-named config surface plus pinned default-365 assertions. Sound reuse,
  not fakery.
- Route/registry invariants hold: no new routes; `/analytics/top` and
  `/analytics/count` were already registered; the added `AuthenticatedAgent`
  extractor on `top_events` matches every sibling analytics handler.
- Compile-plausibility checked by hand against the exact signatures
  (`AnalyticsEvent::new`/`with_scope`, `AnalyticsQueryFilter: Default`,
  `Workspace::new`, `build_router(Arc<AppState>)`, `body_json`, deref
  coercion `&Arc<AppState>` → `&AppState` at the `validate_workspace_scope`
  call — same pattern as the pre-existing call at line 278).

## Findings

- [ ] **F1 (major, blocking): the event-volume bar chart cannot load against
  the real backend — the primary acceptance-criterion section is dead in
  every deployment, and its test is stub-backed so it ships green.**
  `AnalyticsView.loadDaily()` calls `api.analyticsDaily({ days: 30 })`, which
  produces `GET /api/v1/analytics/daily?days=30` — but
  `DailyParams.event_name` (`crates/gyre-server/src/api/analytics.rs:204`)
  is a required, non-`Option` `String` with no `#[serde(default)]`, so axum's
  `Query` extractor rejects the request with 400 before the handler runs.
  Every existing server test passes `event_name` explicitly (analytics.rs:
  2364/2386/2409/2427); the candidate frontend is the first bare caller.
  Section 1 of the dashboard therefore always renders its error state
  ("Failed to load daily event counts"). Deeper: even with an `event_name`,
  the endpoint returns per-event-name daily volume
  (`aggregate_by_day(&params.event_name, …)`), while the spec requires
  **total** daily event volume — so the wiring is wrong at the contract
  level, not merely missing a parameter. `AnalyticsView.test.js` mocks
  `api.analyticsDaily` with `[{date, count}]`, so all 13 tests pass
  regardless — precisely the stub-backed-test class the task goal prohibits.
  Evidence and reproduction:
  `/tmp/stage/review-evidence/daily-chart-contract-mismatch.md`.
  Remedy: make `DailyParams.event_name` optional (None/empty = aggregate
  across all event names, matching the spec's "daily event volume") with a
  server test for the bare `?days=30` form, or add an explicit total-volume
  form the chart uses; passing one event_name is not spec-conformant.

Non-blocking observations:

- Navigation uses the realized two-mode UI model (topbar chart button +
  `/workspaces/:slug/analytics` + mobile drawer) instead of the spec text's
  "sidebar: Analytics under Operations" — the sidebar no longer exists
  (ui-navigation.md, Principle 5); the supersession is documented in the
  coverage notes and docs/ui.md. Acceptable reconciliation, not a finding.
- The `retention_days` analytics form bypasses `validate_policies`, but by
  construction it starts from the store's current (validated) list and only
  sets one field to a value ≥ 1, so no invalid policy can result. Noted, not
  a defect.
- Generated output: the candidate touches no `web/dist/` files (committed
  bundle is stale relative to the new analytics source). Per pipeline
  ownership this is the deterministic verifier's concern — rebuilt and
  committed after integration; not a review finding.

## Verifier must run (blocked offline here)

- `cargo test -p gyre-server analytics`
- `cargo test -p gyre-server retention`
- `cargo check -p gyre-server` / clippy (verification-owned anyway)
- Live probe after the F1 fix: `GET /api/v1/analytics/daily?days=30` (or the
  total-volume form) must return 200 with 30 day buckets, and the Analytics
  view's section 1 must render bars, not the error state.
