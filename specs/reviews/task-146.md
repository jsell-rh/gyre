# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate: `f6bce62e544481eb9879add5dedc3557cf3ba6b8` (branch `pipeline/task-146/594ef731087744868b04f4843e0a66c9-1`), base `276918d2` (main task-128).
Verdict: **needs-revision** — one moderate finding (F1, `group_by` aggregates a limit-truncated event set); everything else verified clean against the task contract.

## Round 1 (2026-11-13)

Probes run (evidence: `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain analytics` → exit 0 (3 passed, incl. the spec-schema pin `analytics_event_matches_spec_schema`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0 (candidate tree type-checks).
- `scripts/check-arch.sh`, `check-migration-versions.sh`, `check-abac-route-registry.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh` → all exit 0.
- Full per-event gyre-server test list recorded for the independent verifier in `probes.log` (the large server test build is excluded from review per the 4 GiB budget rule); the tests were inspected individually instead.

Verified working (no findings):

- **Event schema.** `AnalyticsEvent` (gyre-domain/src/analytics.rs) now carries all 9 spec fields; `with_scope` populates user/session/workspace/repo. `Option<String>` for id-typed fields is a transparent representation of the spec's `Option<Id>` (`Id` is a `String` newtype) and matches the storage layer's TEXT columns — documented in the coverage matrix. Migration `2026-10-08-000063_analytics_scope_columns` uses the next unused sequence number (unique) and portable SQL (ADD COLUMN / CREATE INDEX IF NOT EXISTS work on both SQLite and PG).
- **All 12 auto-emitted events fire at real trigger points with the spec-required properties**, verified by direct inspection of every emission site (not by trusting the Shipped list):
  - `task.status_changed` (old/new/task_id/assigned_to): HTTP endpoint (tasks.rs), MCP `update_task` (old status captured before the transition), spawn auto-advance, completion auto-advance, admin force-kill block, stale-abort, spec-reject cancel, repo-archive cancel, ralph-loop re-open and iteration-ceiling/terminal blocks — all call the shared `record_task_status_changed` after persisting.
  - `mr.merged` (mr_id/repo_id/gate_count/queue_wait_secs): shared `emit_mr_merged` on the queue merge path (single-entry and atomic-group, deferred past rollback) and the HTTP transition path.
  - `mr.closed` (mr_id/repo_id/reason): HTTP transition, spec-reject close, ralph-loop gate-failure close.
  - `agent.spawned` (agent_id/task_id/compute_target/persona): worker spawn reports the bound persona slug (not a hardcoded label) and `launch.compute_target_label`; orchestrator spawn reports the resolved target and an honest `task_id: null` for task-less orchestrators; ralph-loop re-spawn reports the same shape.
  - `agent.completed` / `agent.failed`: HTTP complete/fail/stop, MCP `gyre_agent_complete`, admin kill, stale-abort, spec-reject stop, compose teardown, ralph-loop ceiling and terminal re-spawn failure, orchestrator death monitor — each carries task_id/reason/duration context.
  - `merge_queue.processed` (mr_id/outcome/wait_secs): merged path plus every terminal Failed path routed through `fail_queue_entry`/`emit_queue_processed_terminal` (candidate-selection gate failure, MR/repo not found, spec policy, conflicts, git errors), circuit-breaker Cancelled and operator cancel as outcome "skipped".
  - `gate.failed`/`gate.passed` (gate_id/gate_type/mr_id/output_snippet|duration_secs): emitted in `gate_executor` after real execution, scoped via the MR lookup, with the unresolvable-MR case skipping scope rather than fabricating a default.
  - `spec.approved` (spec_path/approver_type/approval_mode): in the approve handler after the approval event persists, attributed to the approver identity.
  - `budget.warning` (workspace_id/metric/threshold_pct): from the spawn budget check and usage-recording paths at the 80% threshold, with a below-threshold negative test.
  - `search.query` (query_length/entity_types/result_count/duration_ms): HTTP handler and MCP `gyre_search`, both through the shared caller-bound `record_search_query`.
- **Caller-bound identity and scope validation.** `record_event` ignores body `agent_id`/`user_id` and derives them from the verified caller; `workspace_id`/`repo_id` are validated against the caller's tenant via the shared audit validators (cross-tenant → 403, unknown → 404, inconsistent ws/repo pair → 403). Regression tests pin the forged-attribution repro end-to-end (victim scope's filtered results stay empty). The search endpoints apply the same rule because `workspace_id` doubles as the event's workspace attribution.
- **Query API.** All spec filter parameters are implemented on SQLite, Postgres, and mem adapters (`query_filtered`): exact and trailing-`*` event_name, agent/user/workspace/repo ids, since/until (ISO8601 incl. offsets and date-only forms, or unix seconds; inclusive bounds), limit (default 100, cap 10 000), group_by with unsupported fields rejected. `/daily` supports the spec's `?event_name=&days=30` form and no longer 400s without since/until. The adapter test seed is deliberately cross-cutting so dropping any single predicate changes the asserted result sets — not mirrored-logic.
- **Tests are real.** Per-event tests drive the production handlers (HTTP or MCP over the real router) and assert the persisted events and their spec-required properties via `state.analytics.query`; negative tests (below-threshold budget warning, empty search query, forged identity/scope) kill the corresponding fake-completion paths.
- **No new endpoints** (api/mod.rs untouched, ABAC registry gate passes); exemption files unchanged in count (comment-only edits); no invariants violated by the mechanical checks run above.
- Rebase reconciliation with main `276918d2` (task-128) composes: the ABAC/policy additions in the intersection files are additive and every analytics emission still sits on a live trigger path; the one conflict (`spec-links.md` coverage row) was resolved to the value matching the merged tree.

Findings:

- [-] **F1 (moderate): `group_by` aggregates the limit-truncated event set — grouped counts are silently wrong once a scope holds more than `limit` events.** In `query_events` (crates/gyre-server/src/api/analytics.rs:299-365), the event-level `limit` (default 100, cap 10 000) is applied by `query_filtered` (SQL `LIMIT` / mem truncate) *before* the in-memory group_by counting, so `GET /api/v1/analytics/events?group_by=event_name` returns counts over only the newest 100 events by default. Repro: seed 150 `mr.merged` events; `?group_by=event_name` → `{"mr.merged": 100}` while `GET /api/v1/analytics/count?event_name=mr.merged` → 150 — the two endpoints disagree on the same question. The spec types `limit` as "Max results" (for a grouped response the results are the groups) and §Purpose makes the analytics agent-consumable for autonomous decisions, so silently truncated aggregates feed wrong data into exactly that loop; `/count` and `/daily` aggregate server-side without a limit, confirming the intended semantics. No test covers group_by beyond the limit.
  **Repair path:** aggregate server-side (a port-level grouped-count query like `aggregate_by_day`, with `limit` bounding the number of returned groups), or at minimum run the grouped query without the event-level truncation and document the interaction; add a regression test seeding > `limit` events and asserting exact grouped totals consistent with `/count`.

Minor (non-blocking, recorded for completeness):

- SQLite `LIKE` is ASCII-case-insensitive while Postgres `LIKE` is case-sensitive, so a trailing-`*` `event_name` filter behaves slightly differently across backends (exact match is `=` on both). Event names are lowercase by convention; not blocking.
- The task file's Verification evidence cites `/tmp/stage/review-evidence/probes.log` from the implementation sandbox, which does not exist in this checkpoint; the verifier test list is re-recorded in this review's evidence directory.
- No frontend sources changed in this candidate, so no `web/dist` regeneration is owed by this task (the committed bundle is owned by the deterministic verifier regardless).

Test-inflation check: none found. The per-event tests drive real handlers and assert persisted events; the adapter filter test is cross-cutting so each predicate is individually load-bearing; negative tests exist for the forgery and threshold paths.

Scope check: diff touches analytics emission sites, the analytics adapters/port/domain, caller-scope validation sharing, and test-flakiness hardening in rate-limit/middleware/recovery tests carried by the branch (clock injection for the rate-limit race, a warn log on the already-fail-closed recovery path — no enforcement weakened). The `orchestrator_spawn_denied_across_tenants` integration test is a repair of the rebase intersection with main's orchestrator changes and is containment-positive.
