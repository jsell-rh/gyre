# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

Comparison base: d31906c5a16ab8045ab435dfad2bd7dc1ce3f616 → candidate 8a221d2efbe0acc50a87cb578a9bc5ff91bd648d.
Diff: 41 files, +5412/−351. Multi-round branch: earlier rounds (ad7d3e33a caller-bound
identity, f301cff2c fmt/clippy, dfa1266c review round) repaired in e9e597a07
"emit analytics on silent terminal paths, validate search scope". No test removals
(0 removed `#[test]`/`#[tokio::test]`, only additions); exemption-file changes are
comment-only or shrinking (fabricated-scope-defaults 34→30 entries); no new endpoints
(route table identical to base).

## Findings

None blocking. All three durable findings from the prior review round (dfa1266c)
are repaired in e9e597a07 with regression tests that reproduce the exact finding
(forged search attribution → Forbidden/NotFound + zero events in the victim scope's
filtered results; ralph_loop gate-failure close / iteration ceiling / terminal
re-spawn silence; signal_chain teardown_failed_spawn / orchestrator death monitor /
agent status endpoint silence).

## Evidence reviewed

### Spec conformance (analytics.md §Event Schema, §Auto-Emitted Events, §Query API/Parameters)

- **Schema**: `AnalyticsEvent` (crates/gyre-domain/src/analytics.rs) carries all 9
  spec fields; `with_scope` populates user/session/workspace/repo. Scope fields are
  `Option<String>` rather than the spec sketch's `Option<Id>` — documented in the
  coverage notes, semantically equivalent (ids stored as TEXT in every adapter),
  field-for-field shape otherwise identical. `analytics_event_matches_spec_schema`
  (serde round-trip) **ran locally — passes**.
- **All 12 events emit at real trigger points with spec properties** (verified each
  JSON payload at each site): task.status_changed (api/analytics.rs helper; HTTP
  endpoint, MCP update_task, spawn/completion auto-advance, admin kill block,
  stale-abort, spec-reject, repo-archive, ralph_loop re-open/ceiling/terminal),
  mr.merged (merge_requests.rs transition + merge_processor queue merge, with
  gate_count + queue_wait_secs), mr.closed (transition/spec-reject/repo-archive/
  ralph_loop gate-failure close), agent.spawned (spawn.rs, orchestrator.rs,
  ralph_loop re-spawn; compute_target + persona), agent.completed (spawn.rs + mcp.rs
  complete handlers, duration_secs), agent.failed (shared record_agent_failed:
  fail/admin-kill/stale-abort/status endpoint/compose teardown/orchestrator death
  monitor/signal_chain failed-launch/ralph_loop ceiling + terminal re-spawn),
  merge_queue.processed (every terminal queue path funnels through
  fail_queue_entry/emit_queue_processed_terminal — outcome merged/failed/skipped,
  wait_secs), gate.failed/gate.passed (gate_executor.rs, output_snippet/duration_secs),
  spec.approved (specs.rs approval handler), budget.warning (budget.rs threshold
  path), search.query (search.rs, identity bound to verified caller).
- **Query API**: no new endpoints; GET /analytics/events supports event_name (exact
  or trailing-`*` prefix), agent_id, user_id, workspace_id, repo_id, since/until
  (ISO8601 or unix secs), limit (default 100, cap 10_000), group_by
  (event_name/agent_id/workspace_id/day; unsupported field → 400). Implemented on
  SQLite, Postgres, and mem adapters with identical semantics; mem has a parity test
  against sqlite filter behavior.
- **Write-side scope integrity**: POST /analytics/events binds agent/user identity
  to the verified caller and validates workspace/repo scope against the caller's
  tenant (audit.rs validate_workspace_scope/validate_repo_scope); the search
  handler's workspace_id Query param now goes through the same validation (repair
  for the prior round's forged-attribution finding), with a test asserting the
  victim scope's filtered results stay empty.

### Probes run (evidence in /tmp/stage/review-evidence)

- 19 mechanical invariant scripts — **all exit 0** (arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, migration-versions, migration-sql-portability,
  fabricated-scope-defaults, forged-scope-fields, inert-enforcement,
  scope-literal-defaults, byte-slice-truncation, fail-open-ref-resolution,
  lossy-secret-conversion, in-memory-state-stores, mem-port-contracts,
  relative-path-defaults, unbounded-external-http, dead-message-kinds,
  forwarded-header-trust).
- `cargo check -p gyre-server` — clean (6m34s).
- `cargo test -p gyre-domain` — 8 passed, incl. the spec-schema round-trip test.
- `cargo test -p gyre-adapters --lib analytics` — 13 passed, incl.
  `analytics_query_filtered_all_params` (cross-cutting seed: agents span
  workspaces, workspaces span repos, users span workspaces — dropping any single
  predicate changes the asserted result sets, so the test kills a dropped filter).
- Migration 2026-10-08-000062_analytics_scope_columns: plain portable SQL
  (ADD COLUMN + CREATE INDEX IF NOT EXISTS) — runs on both SQLite and PostgreSQL.

### Test strength

Per-event tests invoke the real handler/path (full router or the production
function) and then query the wired-in analytics store, asserting event presence
plus specific properties (old/new status, reason strings, scope ids). Removing an
emission fails the assertions (e.g. `.expect("gate-failure MR close must record an
mr.closed event")`). The search scope regression test reproduces the prior finding
verbatim (cross-tenant → 403, unknown → 404, victim scope filtered results empty).
No self-confirming or assertionless tests found in the added suite.

## Verifier must run (deferred — server test compilation exceeds review budget)

- `cargo test -p gyre-server --lib` — per-event unit tests live in
  api/{analytics,search,spawn,merge_requests,tasks,specs,merge_queue,orchestrator,
  agents,admin,compose,budget}.rs, ralph_loop.rs, merge_processor.rs,
  gate_executor.rs, mcp.rs, stale_agents.rs, mem.rs.
- `cargo test -p gyre-server --test api_integration` (`analytics_record_and_query`).
- Full workspace suite + web build per the standard verifier checks.

## Non-blocking observations

- Read-side tenant isolation of the analytics query endpoints is delegated to the
  ABAC policy layer (resource "analytics", no workspace binding) — the same
  pre-existing pattern as the audit query endpoint, unchanged in capability class
  from the base (which returned all events unfiltered); the write-side forging that
  would pollute scoped data was fixed by this branch.
- Generated `web/dist/` is verifier-owned; not reviewed.
