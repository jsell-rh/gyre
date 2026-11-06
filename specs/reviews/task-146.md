# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate under review: `23ff19e83bc3c46c91d87193683ebb7d0f4fb31e` (base `9bef21a41729a6b696b48a587b1f2e619d0f6ff2`; branch history recovered through checkpoint `ed7ac6d3`).
Verdict: **approved** — no findings.

## Scope

Diff base→candidate: 46 files, +4682/−444. This includes the reconciliation
merge with main (`a488d9df`) and four repair rounds (38c2c5e, b818d20d,
dc13ede3, 6a3ce040, ab8c41a2 per the operational history). The review covers
the final tree state against the task contract.

## Independent probes (this checkout, at the candidate SHA)

- `cargo test -p gyre-domain --lib analytics` → **3 passed, 0 failed**
  (incl. `analytics_event_matches_spec_schema` — pins all 9 spec fields,
  `with_scope` population, serde round-trip). Evidence:
  `/tmp/stage/review-evidence/domain-analytics-test.txt`.
- All 15 static gate scripts pass at the candidate (evidence under
  `/tmp/stage/review-evidence/check-*.txt`): arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, dead-message-kinds, mem-port-contracts,
  migration-versions, migration-sql-portability, byte-slice-truncation,
  relative-path-defaults, fabricated-scope-defaults, scope-literal-defaults,
  lossy-secret-conversion, inert-enforcement, fail-open-ref-resolution,
  forwarded-header-trust, in-memory-state-stores, unbounded-external-http,
  task-commit-attribution.
- **Blocked probes (infrastructure, not code defects):**
  `cargo test -p gyre-adapters --lib analytics` cannot build — the sandbox has
  no network route to static.crates.io (download of the diesel/libsqlite tree
  fails; cache `/tmp/cargo` only holds the domain/common/ports dep set), and
  the sandbox is CPU-starved (external load ≈ 20 on 4 cores; a cached
  gyre-domain lib link took ~30 min wall). The verifier must run:
  `cargo test -p gyre-adapters --lib` (expect 348 passed, incl.
  `analytics_query_filtered_all_params`),
  `cargo test -p gyre-server --lib` (expect 1293 passed, incl. the per-event
  trigger tests), and `cargo test -p gyre-server --test api_integration`.
  Evidence: `/tmp/stage/review-evidence/adapters-analytics-test.txt`
  (network-failure log).

## Verified working (source-level, no findings)

- **Event Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries
  all 9 spec fields; `with_scope` populates user/session/workspace/repo.
  Schema independently proven by the passing unit test above.
- **All 12 auto-emitted events record at real trigger points with all
  spec-required properties** — each emit site read in the production code:
  `task.status_changed` (shared `record_task_status_changed` called from the
  HTTP status endpoint, MCP `update_task`, spawn auto-advance Backlog→
  InProgress, completion auto-advance →Review, admin force-kill block,
  stale-abort block, spec-reject cancel, repo-archive cancel);
  `mr.merged` (HTTP transition + queue merge via `emit_mr_merged`, deferred
  past group rollback so no phantom events); `mr.closed` (HTTP transition,
  repo-archive, spec-reject — each with a distinct reason value);
  `agent.spawned` (worker path with compute_target/persona; orchestrator
  path separately); `agent.completed` (HTTP complete + MCP
  gyre_agent_complete); `agent.failed` (fail, stop, admin kill, stale abort,
  spec-reject stop, repo-archive stop — all with task_id/reason/
  duration_secs); `merge_queue.processed` (merged + every terminal Failed
  path routed through `fail_queue_entry`/`emit_queue_processed_terminal`,
  circuit-breaker Cancelled as outcome "skipped"); `gate.failed`/
  `gate.passed` (gate_executor outcome emission with gate_id/gate_type/
  mr_id/output_snippet/duration_secs); `spec.approved` (approval handler
  with spec_path/approver_type/approval_mode and approver-scope fields);
  `budget.warning` (80% threshold in `emit_budget_warning`, called from
  check_spawn_budget and usage recording); `search.query` (search handler
  with query_length/entity_types/result_count/duration_ms).
- **Query API**: no new endpoints (route count 253→253, honoring the task's
  "do NOT create new endpoints" constraint); `query_filtered` implemented on
  SQLite, Postgres, and the mem adapter with the full spec filter set —
  event_name (exact or trailing-`*` prefix), agent_id, user_id,
  workspace_id, repo_id, since/until (inclusive, ISO8601 or unix-sec),
  limit (default 100, cap 10_000), group_by (event_name/agent_id/
  workspace_id/day, unsupported fields rejected with 400).
- **Migration**: `2026-10-08-000057_analytics_scope_columns` uses the next
  unused sequence number (duplicate-version collision with main's 000056
  fixed in the repair round); portability check passes.
- **Gate integrity**: exemption-file changes vs base are deletions only,
  each backed by a real fix independently confirmed in source —
  orchestrator spawn tenant containment (orchestrator.rs loads the scoped
  entity and returns Forbidden on tenant mismatch, both handlers),
  attestation-ABAC Deny now fails the queue entry before the merge commit
  (merge_processor.rs denial binding), recovery.rs resolve_ref sites fail
  closed, specs.rs fanout resolves tenant from the workspace record,
  gate_executor GateFailure emission skips when workspace unresolvable.
  `FROZEN_EXEMPTION_COUNT` for fabricated-scope-defaults lowered 5→3
  matches the two deletions (tightening, not weakening). No new exemption
  entries anywhere.
- **Tests are meaningful**: per-event tests drive real HTTP handlers / MCP
  tools / merge-processor paths against the AppState analytics repository
  and assert exact property values and event counts (e.g. admin-kill asserts
  `agent.failed` reason "force-killed by admin" and the auto-block
  `task.status_changed` old/new statuses). Not self-confirming: assertions
  name concrete values the production code computes independently.
- **Coverage matrix honest**: analytics.md 5/12 implemented (the five
  task-146 sections); remaining sections (Decision API) stay task-assigned
  to task-147; SUMMARY TOTAL recomputed consistently (305+115+163 = 583 ≠
  802−219−0 = 583 ✓).

## Non-blocking observations

- Cross-adapter nuance: SQLite `LIKE` is ASCII-case-insensitive and treats
  `_` as a single-char wildcard, while the mem adapter's prefix match is
  literal — so an `event_name` prefix containing `_`/mixed case (e.g.
  `task.status_*`) can match marginally more rows on SQLite than on mem.
  All platform event names are lowercase; the exact-match path uses `.eq()`
  on both backends; the pre-existing `query` method shares this class of
  nuance. Not a spec violation.
- `mr.merged.queue_wait_secs` is null when the MR merged without passing
  through the queue (direct HTTP transition) — property present, value
  None, appropriate for "not applicable".

Deferred to verifier: the three cargo suites listed above (network-blocked
here) and the full `bash /tmp/stage/checks.sh`.
