# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

**Candidate:** `d5bf01c2bef6492568846b2ad8f8ec12a01c99b1`
**Base:** `f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3`
**Verdict:** Approved (independent evidence below; no blocking findings).

## Scope

`specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events,
§Query API, §Query Parameters — the task's five coverage sections. The
Decision API sections (6–12) are task-147 and remain `task-assigned`
(correctly out of scope; the task forbids new endpoints).

## What was verified (source-level, production code only)

1. **Event Schema** — `gyre-domain/src/analytics.rs` `AnalyticsEvent` carries
   all 9 spec fields (`id`, `event_name`, `agent_id`, `user_id`,
   `session_id`, `workspace_id`, `repo_id`, `properties`, `timestamp`);
   `with_scope()` populates the scope ids. Id-typed fields are stored as
   `Option<String>` (documented storage-layer representation, consistent
   with the pre-existing `agent_id`), and the serde round-trip test
   `analytics_event_matches_spec_schema` pins the shape. Not a functional
   gap: ids are stringified uniformly at every emission site.
2. **All 12 auto-emitted events** fire at real trigger points with all
   spec-required properties (verified by direct inspection of every
   production emission site, not test code):
   - `task.status_changed` — helper + HTTP status endpoint, MCP
     `update_task`, spawn/completion auto-advance, admin force-kill,
     stale-abort, spec-reject, repo-archive, ralph-loop re-open/ceiling/
     merge-completion, signal-chain scheduler claims/releases.
   - `mr.merged` — queue merge path (`emit_mr_merged`) and HTTP transition
     endpoint; `gate_count` from real gate_results, `queue_wait_secs` from
     real enqueue time.
   - `mr.closed` — HTTP transition, repo-archive, spec-reject, ralph-loop
     gate-failure close; each carries `reason`.
   - `agent.spawned` — worker spawn + orchestrator spawn, with resolved
     `compute_target` label and bound persona (honest
     `default-worker`/orchestrator labels for unbound personas).
   - `agent.completed` — HTTP complete and MCP `gyre_agent_complete`.
   - `agent.failed` — API fail/stop, MCP, ralph-loop ceilings, worker
     failed-launch, orchestrator death monitor, stale-abort, budget kill.
   - `merge_queue.processed` — merged plus every terminal
     failed/skipped path (`emit_queue_processed_terminal`).
   - `gate.failed`/`gate.passed` — `gate_executor` pre- and post-merge
     evaluation, with `output_snippet`/`duration_secs`.
   - `spec.approved` — specs approval handler, with approver identity
     resolution.
   - `budget.warning` — spawn check and usage-recording paths, metrics
     `agents`/`tokens`/`cost`, 80% threshold.
   - `search.query` — HTTP search and MCP `gyre_search`, identity bound to
     the verified caller and workspace scope validated against the
     caller's tenant before attribution.
3. **Query API / Query Parameters** — no new endpoints (as instructed);
   `GET /events` supports `event_name` (exact + trailing-`*` literal
   case-sensitive prefix), `agent_id`, `user_id`, `workspace_id`,
   `repo_id`, `since`/`until` (ISO8601 or unix secs, inclusive,
   date-only upper bound extends to end-of-day), `limit` (default 100,
   cap 10_000), and `group_by` (`event_name`/`agent_id`/`workspace_id`/
   `day`) aggregated server-side over the FULL filtered set with `limit`
   bounding groups. `/daily` supports the spec's `?event_name=&days=`
   form. Prefix semantics are literal and case-sensitive on all three
   adapters (mem `starts_with`, SQLite `substr(...) = ...` under BINARY
   collation, Postgres escaped case-sensitive LIKE); UTC day grouping on
   Postgres via `AT TIME ZONE 'UTC'`.
4. **Anti-forgery on the record path** — `record_event` ignores
   body-supplied `agent_id`/`user_id` (binds to the verified caller,
   impersonation attributed to the impersonator) and validates
   `workspace_id`/`repo_id` against the caller's tenant (403 cross-tenant,
   404 unknown, 403 on workspace/repo inconsistency), with regression
   tests asserting the forged event never reaches the victim scope's
   filtered results.
5. **Migration** `2026-10-08-000063_analytics_scope_columns` — sequence
   63 is the next unused number; SQL is portable (plain `ALTER TABLE ADD
   COLUMN`, `CREATE INDEX IF NOT EXISTS`). The 2026-10-08 date prefix
   sorts below already-applied 2026-11-* versions; verified against
   diesel_migrations 2.3.1 source (`migration_harness.rs`):
   `pending_migrations` is set-subtraction of applied versions, sorted
   ascending — a lower-versioned new migration still runs on upgraded
   databases, and on fresh databases the ALTER only depends on the
   initial-schema `analytics_events` table. Not a defect.
6. **Test killing power** — mutation probe: disabling the SQLite
   `workspace_id` filter in `query_filtered` makes
   `analytics_query_filtered_all_params` FAIL (15 passed / 1 failed);
   restored afterwards and re-verified 16/16 green. The filter/grouping
   tests are not self-confirming.
7. **Coverage matrix** — `specs/coverage/system/analytics.md` honestly
   marks sections 1–5 `implemented` with per-section evidence and leaves
   6–12 `task-assigned` for task-147.

## Probes run (logs in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain` — 508 passed (incl.
  `analytics_event_matches_spec_schema`).
- `cargo test -p gyre-adapters analytics` — 16 passed (filter params,
  prefix literal case-sensitivity, grouped counts over full set,
  delete_older_than, cost suites).
- Mutation probe (workspace filter disabled → test fails; restored →
  green).
- `cargo check -p gyre-server --tests` — clean (all server test code
  compiles).
- `cargo check -p gyre-cli -p gyre-ports -p gyre-domain -p gyre-common` —
  clean.
- `cargo fmt --check` — clean.
- Mechanical gates all pass: check-arch, check-hierarchy(ABAC registry),
  check-abac-exempt-handlers, check-mcp-write-tools,
  check-migration-versions, check-migration-sql-portability,
  check-dead-message-kinds, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-task-commit-attribution (against the committed task file — see
  note), check-mem-port-contracts, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-forged-scope-fields,
  check-forwarded-header-trust, check-in-memory-state-stores,
  check-unbounded-external-http.

## Deferred to the independent verifier (gyre-server test execution)

The build wrapper defers gyre-server test runs to the verifier. Execute:

- `cargo test -p gyre-server api::analytics`
- `cargo test -p gyre-server gate_executor`
- `cargo test -p gyre-server merge_processor`
- `cargo test -p gyre-server ralph_loop`
- `cargo test -p gyre-server signal_chain`
- `cargo test -p gyre-server stale_agents`
- `cargo test -p gyre-server mem::` (incl.
  `query_filtered_matches_sqlite_filter_semantics`)
- `cargo test -p gyre-server api::budget` / `api::specs` / `api::search` /
  `api::tasks` / `api::merge_requests` / `api::merge_queue` / `mcp`
- `cargo test -p gyre-server --test api_integration`
  (`orchestrator_spawn_denied_across_tenants`)

Postgres adapter SQL is compile-checked only (no PG server in this
sandbox); the same constructs (escaped LIKE, `AT TIME ZONE 'UTC'`,
alias GROUP BY — valid in PostgreSQL) are exercised by the SQLite/mem
adapter tests.

## Non-blocking observations (follow-ups, not task-146 defects)

- `scripts/scope-literal-defaults-exemptions.txt` references
  `sqlite/analytics.rs:115/225`, but the pre-existing
  `tenant_id: "default"` insert literals moved to lines 154/427 after
  this branch's insertions. The check is only invoked without explicit
  filenames (pre-commit and dev-check both use `pass_filenames: false`),
  under which it scans nothing — pre-existing script brittleness, not a
  candidate regression; the candidate added no exemption entries.
- The analytics GET endpoints are not tenant-scoped (any authenticated
  caller can filter by any workspace_id/repo_id). This exposure
  predates task-146 (the GET routes returned all events at base); the
  new filters narrow rather than broaden it, and the record path now
  rejects forged cross-tenant attribution. Worth a follow-up task to
  scope reads to the caller's tenant like the search endpoints do.
- `budget.warning` re-fires on every budget check once usage is at/above
  the threshold rather than only at the crossing edge — "threshold
  crossed" semantics are arguably satisfied; deduplication could be a
  refinement.

## Note on working-tree state

The controller reset `specs/tasks/task-146.md` in the working tree
(progress `not-started`, `commits: []`) for this review round; the
committed candidate `d5bf01c2` carries the full attribution list, and
all 22 `fix/feat/test(task-146)`-labeled commits in `base..candidate`
are present in its `commits:` frontmatter (verified against the
committed file; the check-task-commit-attribution failure against the
working tree is that reset artifact, not missing attribution).
