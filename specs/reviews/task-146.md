# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted
Events, §Query API, §Query Parameters.
Candidate: `5c4ff9b7e620eca7e17493628842f3194b4ed5bd` (base
`41f4d56e8419e24a2a66c7fab33e79445d4a656b`).
Verdict: **approved** — all five coverage sections are backed by real
production code; no blocking findings.

## Round 1 (2026-11-12) — independent verification

Probes run in the review sandbox (evidence: `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain analytics` → 3/3 passed (spec-schema struct test
  with serde round-trip).
- `cargo test -p gyre-adapters analytics` → 13/13 passed, including
  `analytics_query_filtered_all_params`: a cross-cutting seed (agents span
  workspaces, workspaces span repos, users span workspaces) makes each scope
  predicate non-redundant, so dropping any one filter predicate from
  `query_filtered` changes the asserted result sets.
- `cargo check -p gyre-server` → clean.
- All mechanical gates exit 0: migration versions, migration SQL portability,
  arch, fabricated-scope-defaults, inert-enforcement, mem-port-contracts,
  scope-literal-defaults, mcp-write-tools, abac-route-registry,
  abac-exempt-handlers, dead-message-kinds, byte-slice-truncation,
  forged-scope-fields, task-commit-attribution (against the committed
  candidate frontmatter — 16 SHAs, complete).
- gyre-server test compilation intentionally NOT run in the review sandbox
  (4 GiB budget — verifier responsibility). Required server probes for the
  verifier: `cargo test -p gyre-server analytics`, `cargo test -p gyre-server
  query_filtered`, `cargo test -p gyre-server analytics_event`,
  `cargo test -p gyre-server queue_processed`, `cargo test -p gyre-server
  budget_warning`, then the full workspace suite per AGENTS.md.

Verified against the task contract:

- **Event Schema (AC 1).** `AnalyticsEvent` (gyre-domain/src/analytics.rs)
  carries all 9 spec fields; `with_scope` populates
  user/session/workspace/repo. Migration `2026-10-08-000063` adds the four
  scope columns + indexes to `analytics_events` with portable SQL (runs on
  SQLite and PostgreSQL); SQLite, Postgres, and mem adapters all persist and
  round-trip the fields. Domain unit test pins the shape.
- **All 12 auto-emitted events at their trigger points (AC 2).** Every event
  name has real production emission sites — read each site in the diff:
  `task.status_changed` (HTTP status endpoint, MCP update_task, spawn
  assign/completion auto-advance, admin force-kill, stale-abort, spec-reject,
  repo-archive, ralph-loop re-open/ceiling/respawn — shared
  `record_task_status_changed` helper carries old/new status and assigned_to);
  `mr.merged` (HTTP transition + queue merge via `emit_mr_merged`, deferred
  until after the rollback-capable merge loop so a group rollback cannot
  leave a phantom merged event); `mr.closed` (transition endpoint, repo
  archive, spec reject, ralph-loop gate-failure close); `agent.spawned`
  (worker spawn, orchestrator spawn with the *resolved* compute-target label,
  ralph-loop re-spawn — persona reports the bound persona slug, with a vacuity
  guard in its test); `agent.completed` (HTTP complete endpoint + MCP
  gyre_agent_complete); `agent.failed` (fail/stop endpoints, MCP, admin kill,
  stale abort, spec reject, repo archive, ralph-loop ceiling/respawn-failure,
  worker failed-launch teardown, orchestrator process-death monitor); every
  merge-queue failure path is funneled through `fail_queue_entry` (16 call
  sites) so the Failed status and the `merge_queue.processed` event cannot
  diverge (outcome merged/failed/skipped); `gate.failed`/`gate.passed` in
  gate_executor's `run_gate`; `spec.approved` in the approval handler with
  approver identity split agent/human; `budget.warning` on the spawn-budget
  check (agents) and usage recording (tokens/cost) at the 80% threshold;
  `search.query` in the search handler with caller-bound identity.
- **Spec-required properties (AC 3).** Each emission site includes the full
  property set from the spec table (verified property-by-property at every
  site; extra context properties like `duration_secs` on agent.failed are
  additive, not substitutions).
- **Query API / Query Parameters (AC 4).** No new endpoints (routes and ABAC
  registry unchanged, per the task constraint). GET /analytics/events now
  supports the full spec filter set: `event_name` (exact or trailing-`*`
  prefix via LIKE on SQLite and PG), `agent_id`, `user_id`, `workspace_id`,
  `repo_id`, `since`/`until` (ISO8601 incl. offsets and date-only forms, or
  unix seconds; inclusive bounds; date-only upper bound extends to end of
  day), `limit` (default 100, cap 10_000), `group_by`
  (event_name/agent_id/workspace_id/day; unsupported field → 400). GET
  /daily supports the spec's `?event_name=&days=30` form with until defaulting
  to now. All three adapters implement the new `query_filtered` port method.
- **Tests (AC 5).** Per-event tests assert the exact property values, not
  just event presence, and include genuine negative controls (no
  budget.warning below threshold; no gate.passed for a failing gate; no
  search.query for an empty query; forged cross-tenant attribution leaves the
  victim scope's filtered results empty) and vacuity guards (persona-binding
  sanity check before the analytics assertion). Adapter-level filter tests
  use a cross-cutting seed so each predicate is independently load-bearing.

Hardening carried from earlier review rounds (verified still present):

- `record_event` derives `agent_id`/`user_id` from the verified caller and
  validates `workspace_id`/`repo_id` against the caller's tenant
  (Forbidden on cross-tenant, NotFound on unknown, inconsistent
  workspace/repo pair rejected) — a caller cannot forge scope attribution
  that would pollute another scope's filtered results.
- `search_handler` binds the `search.query` event identity to the caller and
  validates the workspace scope it attributes.

Findings: none blocking.

Non-blocking observations (recorded for future task owners, not this
contract):

- Read-side tenant scoping of GET /analytics/events is pre-existing platform
  behavior (the base's query path had no tenant filter either); the candidate
  does not regress it — the new parameters only narrow results, and the write
  path was hardened. If analytics reads should be tenant-scoped, that is a
  separate task against §Query API.
- The working tree (not the candidate commit) carries a controller-side
  restore of `specs/tasks/task-146.md` to the assignment baseline; the
  committed candidate frontmatter is complete (attribution gate exit 0
  against it).
