# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters.
Candidate under review: `b3cdf76e0` (base `b5a872087`); task commits `3bcd6f525`, `d7641ddd`, `8f0ee190`, `ed7ac6d3`.
Verdict: **needs-revision** — one material finding (F1, weak sqlite adapter test); the rest of the task contract verified real by inspection and mutation probes.

## Probes run (candidate tree, `/tmp/stage/bin/cargo`)

- Offline gates, all exit 0: migration-versions, arch, fabricated-scope, inert-enforcement, ABAC registry + exempt handlers, scope-literal, forged-scope, forwarded-header, in-memory-state-stores, mem-port-contracts, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, lossy-secret-conversion, mcp-write-tools, task-commit-attribution.
- `cargo test -p gyre-server --lib analytics` → 42 passed / 0 failed (per-event emission tests for all 12 spec events).
- `cargo test -p gyre-server --lib queue_processed` → 7 passed / 0 failed (terminal queue-path emissions).
- `cargo test -p gyre-server --lib budget_warning` → 2 passed / 0 failed (positive + below-threshold negative control).
- `cargo test -p gyre-server --test api_integration orchestrator_spawn` → `orchestrator_spawn_denied_across_tenants ... ok` (prior round's containment regression, post-fix).
- `cargo test -p gyre-adapters --lib analytics` → 13 passed / 0 failed.
- `cargo test -p gyre-domain --lib analytics` → 3 passed / 0 failed (incl. `analytics_event_matches_spec_schema` serde round-trip).
- Mutation probes in a scratch worktree at the candidate commit (main checkout untouched, `git status` clean throughout; patches and logs in `/tmp/stage/review-evidence/README.md`).

## Verified real (no findings)

- **Event schema.** `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields; `with_scope` populates user/session/workspace/repo; id-typed fields stored as `Option<String>` matching the storage TEXT columns (documented in coverage row 2); serde round-trip test pins the shape.
- **All 12 auto-emitted events fire at real trigger points with all spec-required properties** (every emission site read): `task.status_changed` (shared helper called from the HTTP status endpoint, MCP `update_task`, spawn/completion auto-advance, admin force-kill, stale-abort, spec-reject, repo-archive), `mr.merged`/`mr.closed` (HTTP transition with real `gate_count` lookup and queue-derived `queue_wait_secs`; queue merge path; repo-archive; spec-reject), `agent.spawned` (worker path + orchestrator tiers; `default-worker` persona is spec-backed by agent-runtime.md §Bootstrap), `agent.completed` (HTTP + MCP completion paths, duration from `spawned_at`), `agent.failed` (fail/admin-kill/stale-abort/spec-reject stop/repo-archive stop/compose teardown), `merge_queue.processed` (merged + every terminal path via `emit_queue_processed_terminal`, incl. operator-cancel `skipped`), `gate.failed`/`gate.passed` (gate_executor with byte-safe `output_snippet` truncation), `spec.approved`, `budget.warning` (threshold-gated), `search.query` (with empty-query negative control). The `mr.closed` `reason` constant is genuine — the close transition API accepts no reason parameter.
- **Emission tests are not self-confirming.** Mutation-verified: disabling the `search.query` emission, the `record_task_status_changed` helper, or the mem adapter's workspace filter each makes the corresponding test fail (logs: mutation-search-query.log, mutation-task-status-changed.log, mutation-mem-workspace-filter.log). Per-event tests assert exact property values read back through the real query path.
- **Query API.** Handler supports the full spec filter set (plus `user_id` superset), `group_by` with unsupported-field rejection, limit default 100/cap 10_000, ISO8601-or-unix since/until; sqlite, postgres, and mem adapters all implement `query_filtered` with every predicate (code read on all three). API-level scope-filter test `query_events_filters_by_scope_ids` is individually discriminating per parameter (mutation-verified on the mem backend).
- **Migration.** `2026-10-08-000059_analytics_scope_columns` — unique Diesel version, next unused sequence, portable SQL (plain `ALTER TABLE ADD COLUMN` + `CREATE INDEX IF NOT EXISTS`) for both SQLite and Postgres.
- **Containment fix (commit 3bcd6f525).** Workspace-tier orchestrator spawn enforces tenant containment for every caller (no `is_system_principal` bypass); repo-tier keeps the documented dev-bootstrap bypass; both doc comments record the asymmetry. Regression test passes.
- **No fakery in hygiene files.** fabricated-scope exemption count lowered 2 → 1 with the gate_executor site actually fixed (emission skipped + logged on unresolvable MR workspace); inert-enforcement change is comment-only; no new exemption entries anywhere; all offline gates green.

## Findings

### F1 (material): `analytics_query_filtered_all_params` cannot catch a dropped scope predicate on the sqlite adapter

`crates/gyre-adapters/src/sqlite/analytics.rs` — the test's conjunctive block (event_name=mr.merged ∧ agent_id=a2 ∧ workspace_id=ws-2 ∧ repo_id=r1, expect exactly `f2`) seeds data in which **each** of those predicates alone isolates `f2` (`f1`/`f3` are agent `a1`), so every predicate is redundant with the others; the test also never seeds or filters `user_id`. Mutation proof: deleting the `workspace_id` predicate from `SqliteAnalyticsRepository::query_filtered` leaves the test green (`cargo test -p gyre-adapters --lib analytics_query_filtered_all_params` → ok; log: mutation-adapter-workspace-filter.log). SQLite is the default production backend, and coverage row 5 cites this test as the adapter-level proof of the spec-required filter set — a regression dropping a scope predicate there would ship green. The API-level/mem tests are discriminating, so only the sqlite guard is hollow.

Fix: seed cross-cutting data (events sharing an agent but differing workspace; sharing a workspace but differing repo; at least one event with a `user_id`) and assert each scope filter individually, plus a `user_id` filter case.
