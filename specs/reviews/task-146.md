# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

**Candidate:** `e2dfa6b31c69b407508d6c75928db9118bb9369a` (base `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`)
**Reviewer stage:** independent review (retry after infrastructure-model failure)
**Verdict:** **Approved** — no findings.

## What was reviewed

The full diff base→candidate: `AnalyticsEvent` schema extension (domain + ports + SQLite/Postgres/mem adapters + migration `2026-10-08-000056_analytics_scope_columns`), the new `query_filtered` port method with `AnalyticsQueryFilter`, all 12 auto-emitted event trigger points, the query-API parameter surface (`group_by`, ISO8601 `since`/`until`, scope-id filters), and the per-event tests.

## Acceptance criteria — verified with independent evidence

**1. AnalyticsEvent struct matches spec schema.** All 9 spec fields present (`gyre-domain/src/analytics.rs`): id, event_name, agent_id, user_id, session_id, workspace_id, repo_id, properties, timestamp; `with_scope()` populates the four scope ids; `analytics_event_matches_spec_schema` pins the shape incl. serde round-trip (passed). Id-typed fields are `Option<String>` — a pre-existing pattern (`agent_id` was already `String`-typed before this task), documented in the coverage matrix, semantics preserved (ids stored/filtered as text). Not a deviation that hollows the schema.

**2. All 12 auto-emitted events recorded at trigger points** — verified by reading every emission site, with multi-path coverage beyond the task plan:

| Event | Sites |
|---|---|
| `task.status_changed` | HTTP transition (`api/tasks.rs:347`), MCP `gyre_update_task` (`mcp.rs:1064`) |
| `mr.merged` | HTTP transition (`api/merge_requests.rs:666`), queue merge (`merge_processor.rs` `emit_mr_merged`, deferred past atomic-group rollback) |
| `mr.closed` | HTTP transition, repo archive (`api/repos.rs:320`), spec-reject close (`api/specs.rs:938`) |
| `agent.spawned` | worker spawn (`api/spawn.rs:1086`), orchestrator spawn (`api/orchestrator.rs:143`) |
| `agent.completed` | HTTP complete (`api/spawn.rs:1323`), MCP `gyre_agent_complete` (`mcp.rs:1454`) |
| `agent.failed` | fail handler, stop/kill (`api/spawn.rs:1477,1563`), admin kill (`api/admin.rs:339`), stale-agent abort (`stale_agents.rs:43`) |
| `merge_queue.processed` | queue merge (merged + failed outcomes, `merge_processor.rs:759,1536,1823`) |
| `gate.failed` / `gate.passed` | gate execution loop (`gate_executor.rs:104,114`) |
| `spec.approved` | approval handler (`api/specs.rs:703`) |
| `budget.warning` | 80% threshold, wired into `check_spawn_budget` (agents/tokens/cost) and `record_budget_usage` (`api/budget.rs`) |
| `search.query` | search handler (`api/search.rs:84`) |

**3. Spec-required properties on each event.** Read every emission site; all spec-required properties present (`mr.merged`: gate_count + queue_wait_secs; `merge_queue.processed`: outcome + wait_secs; `task.status_changed`: old/new/assigned_to; `agent.spawned`: compute_target + persona; gates: gate_id/gate_type/mr_id + output_snippet or duration_secs; `spec.approved`: spec_path/approver_type/approval_mode; `budget.warning`: workspace_id/metric/threshold_pct; `search.query`: query_length/entity_types/result_count/duration_ms). Multi-path triggers record the same event name with the same required properties.

**4. Query API supports all spec filter parameters.** `GET /api/v1/analytics/events` accepts event_name (exact + trailing-`*` prefix via LIKE), agent_id, user_id (extra), workspace_id, repo_id, since/until (ISO8601 or unix secs; until defaults to now), limit (default 100, cap 10_000 per spec), group_by (event_name/agent_id/workspace_id/day; unsupported fields rejected with 400). `query_filtered` implemented in **all three** adapters (SQLite, Postgres, mem) with identical filter semantics; the mem adapter's filter-matching is pinned to SQLite semantics by test. No new endpoints were created (per the task's explicit instruction); `api/mod.rs` is unchanged, so no new ABAC registry entries were needed.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib`: **1244 passed, 0 failed** — includes a per-event test for every one of the 12 events plus query-parameter tests (scope filters, prefix wildcard, ISO8601 bounds, group_by aggregation, limit truncation).
- `cargo test -p gyre-domain -p gyre-adapters --lib`: **372 + 345 passed, 12 ignored, 0 failed** — includes the spec-schema struct test and `sqlite::analytics::analytics_query_filtered_all_params`.
- **Mutation probes (both directions, then restored; working tree clean afterward):**
  - Disabled the `search.query` emission in `api/search.rs` → `search_emits_analytics_event` **FAILED** as required.
  - Disabled the gate analytics emission in `gate_executor.rs` → both `passing_gate_emits_gate_passed_analytics_event` and `failing_gate_emits_gate_failed_analytics_event` **FAILED** as required.
  - Tests exercise the real handler path (axum `oneshot` through the router / real `process_next` queue processing / real background gate execution), not mirrored logic — they fail when production behavior is removed.
- Mechanical invariants, all rc=0: check-arch, check-migration-versions (000056 is next unused), check-mem-port-contracts, check-mcp-write-tools, check-abac-route-registry, check-abac-exempt-handlers, check-byte-slice-truncation, check-fabricated-scope-defaults, check-inert-enforcement, check-scope-literal-defaults, check-migration-sql-portability (plain `ALTER TABLE ADD COLUMN` / `CREATE INDEX IF NOT EXISTS` — portable to both backends), check-dead-message-kinds, check-task-commit-attribution.
- Exemption-file edits in the diff are line-number relocations of existing entries (same site, shifted lines), not new exemptions — consistent with the passing checks.

## Notes (non-blocking)

- The tests use the mem analytics adapter (the repo's standard `test_state`); the SQLite adapter has its own independent `query_filtered` test, and Postgres follows the repo-wide never-integration-tested pattern covered by the SQL-portability check — same posture as prior approved tasks, not a finding.
- Web/dist and `specs/tasks/task-200.md` changes in the diff are pipeline merge/bookkeeping artifacts, not product-surface changes.
- Coverage matrix updates are honest: sections 1–5 marked `implemented` (not `verified`), decision-API sections 6–12 remain `task-assigned` to task-147 — no side-effect closure.
