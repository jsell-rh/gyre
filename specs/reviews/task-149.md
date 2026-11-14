# Review — task-149 (Implement analytics dashboard UI and data retention policy)

Spec: `specs/system/analytics.md` §Analytics Dashboard (UI) + §Data Retention.
Commits under review: `f2d6db71` (implementation), `af587a17` (repair of round-1 finding); `d9f19aea`/`269ccd00`/`5e33e322`/`7a17b5cc` (chore/process).
Verdict: **approved** (round 2 — round 1's single material finding is repaired and independently re-verified).

## Round 1 finding (repaired)

**F1 — bar-chart contract mismatch (fixed in `af587a17`).** The chart's
`GET /api/v1/analytics/daily?days=30` 400'd because `DailyParams.event_name`
was a required `String`, and even with a name the endpoint returned a
per-name series instead of total daily volume. Repair chain, verified
end to end this round:

- `DailyParams.event_name` → `Option<String>` + `#[serde(default)]`
  (`crates/gyre-server/src/api/analytics.rs`); the bare `?days=30` form
  deserializes (round 1's live probe had proven the required-String was
  the exact 400 cause).
- `daily_events` normalizes absent/empty `event_name` to `None` and calls
  `aggregate_by_day(None, since, until)`; explicit names still filter.
- The port signature changed to `Option<&str>` and ALL implementors were
  updated (SQLite, Postgres, mem, `IndexingAnalyticsRepository`) — no
  adapter left behind; port contract doc updated; `check-mem-port-contracts`
  passes.
- Regression tests at two levels: HTTP
  (`daily_events_bare_days_form_returns_total_volume` — bare form 200 +
  cross-name totals, empty-`event_name` form equivalent, explicit-name
  form filters) and storage
  (`analytics_aggregate_by_day_no_event_name_totals_across_names` — run
  against real SQLite by this review, passed).

## Round 2 probes

- `cd web && npm ci && npx vitest run src/__tests__/AnalyticsView.test.js`
  → **13/13 passed**.
- `cargo check -p gyre-server` → clean (8m39s cold, 0 errors).
- `cargo test -p gyre-adapters aggregate_by_day` → **2/2 passed** on real
  SQLite, including the None-totals repair regression.
- Mechanical checks pass: check-arch, check-mem-port-contracts,
  check-abac-route-registry, check-forged-scope-fields,
  check-inert-enforcement, check-unbounded-external-http,
  check-conditional-test-guards, check-assertionless-tests,
  check-exhaustive-ui-state. check-dead-components reports 4 violations,
  all pre-existing at the base commit and unrelated to this task.
- Evidence: `/tmp/stage/review-evidence/round2-probes.md`.

## Verified working (no findings)

- **All five dashboard sections are real and wired to the live API**
  (`web/src/components/AnalyticsView.svelte`, ~1060 lines):
  1. SVG 30-day bar chart from `GET /analytics/daily?days=30`, zero-filled
     across all 30 UTC calendar days (server day keys are UTC; the
     zero-fill uses `Date.UTC` — consistent).
  2. Recent-events table from `GET /analytics/events?limit=50`: event /
     agent / workspace filters (dropdown options derived from live
     `/top` and `group_by` queries), click-to-expand full properties
     JSON, "Load more" timestamp-cursor pagination with client-side id
     dedupe.
  3. Decision evaluator: all six built-in decisions, editable JSON
     context (validated client-side before the call), `GET /analytics/decide`,
     outcome/confidence/reason rendered ahead of raw metrics.
  4. Funnel: default `agent.spawned → task.status_changed(new_status:InProgress)
     → agent.completed → mr.merged` with per-step continue/drop-off
     percentages, plus custom funnel definitions. All four event names
     are real auto-emitted events (`mr.merged` is emitted by
     merge_processor). Backend support added: `/count` `since`/`until`
     now optional (default 0..now) and `?property=key:value` equality
     filter evaluated in Rust over `query_filtered`; malformed filter → 400.
  5. Top agents via `GET /analytics/top?dimension=agent`: per-agent
     aggregation over `query_filtered` (event count, completed, failed,
     completion rate = completed/(completed+failed) — `null` when no
     terminal events, matching the task formula — and avg
     `duration_secs` over completions). `workspace_id` is
     tenant-validated exactly like `decide` (403 cross-tenant, 404
     unknown — covered by a real HTTP test). Unknown dimension → 400.
- **Data retention**: `PUT /api/v1/admin/retention` accepts the
  analytics-scoped form `{"retention_days": N}` (>= 1 validated; 0 /
  wrong-type / unknown-key bodies → 400 with policies unchanged),
  updates ONLY the `analytics_events` policy (siblings untouched —
  asserted), persists durably before the 204. The complete-list form
  and its `validate_policies` guard are preserved. Nightly 02:00 UTC
  cleanup job + `delete_older_than` hard-delete across SQLite/Postgres/mem
  pre-exist as real enforcement (business-continuity task); default 365
  is now pinned by both the GET endpoint test and
  `default_policies_present`. The dashboard surfaces the configured
  window read-only (hidden for non-admin callers).
- **Navigation**: topbar chart button, `/workspaces/:slug/analytics`
  route (parse + serialize + popstate + initial-load paths), mobile
  drawer link, Esc-back. Consistent with ui-navigation.md's no-sidebar
  two-mode model, which supersedes the analytics.md "sidebar" wording;
  the task plan explicitly authorizes "workspace-scoped equivalent".
  docs/ui.md updated.
- **Tests are real guards, not inflation**: component tests assert
  exact API call shapes (`{days: 30}`, cursor `until`, funnel property
  filter), rendered values (drop-off percentages, completion rates,
  zero-filled 30 bars), and negative paths (invalid JSON context never
  calls the API; malformed funnel step rejected, default funnel
  unchanged). Server tests drive the real router via `oneshot` with
  bearer tokens and assert cross-tenant 403 / unknown 404 / malformed
  400 — they fail if the enforcement is removed.
- **Coverage matrix** (`specs/coverage/system/analytics.md`) honestly
  separates pre-existing retention enforcement (attributed to
  business-continuity task) from this task's delta.

## Non-blocking observations (recorded, not findings)

- `web/src/lib/api.js` now defines `adminRetention` twice in the same
  object literal (pre-existing key at ~line 340, new duplicate at
  ~line 767; identical implementations, last-wins — benign, no lint
  gate covers it). The new `setAnalyticsRetention` helper is unused by
  the UI (retention display is read-only by design; the endpoint is
  the spec requirement).
- "Load more" sets `hasMoreEvents = false` if a page comes back
  entirely duplicate ids (same-second boundary) — a pagination edge
  case, not a spec violation.
- Generated `web/dist/` is untouched (verifier-owned).

## Verifier follow-up

Server test execution deferred to the independent verifier (stage
budget rules); `cargo check -p gyre-server` passed here. Focused list:
`cargo test -p gyre-server analytics::tests::daily_events_bare_days_form_returns_total_volume`,
`...::top_agents_dimension_ranks_and_computes_rates`,
`...::top_agents_workspace_scope_validated_against_caller_tenant`,
`...::count_property_filter_matches_property_equality`,
`cargo test -p gyre-server admin_update_retention`,
`cargo test -p gyre-server retention::tests`,
`cargo test -p gyre-adapters analytics_`, plus standard CI
(`cargo test --all`, `cd web && npm test`, web/dist rebuild) and
`scripts/check-task-commit-attribution.sh` (needs full git history;
frontmatter records both product commits).
