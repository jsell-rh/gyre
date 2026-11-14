# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (+ §Purpose as context).
Candidate under review: `91f22ac102d7d1a6537cba94433b5e87ce886f5a` (branch `pipeline/task-146/021011247ba545f7aa04b6a5649693a6-1`), diffed against assigned base `6ffd679985be9826193f4dc089d9b1f16766273c`.
Verdict: **approved** — all five coverage sections are backed by real production code; no findings.

## Probes run (this round, at candidate `91f22ac1`)

- `cargo test -p gyre-domain analytics` → **3 passed** (incl. `analytics_event_matches_spec_schema`, which pins all 9 spec fields + serde round-trip). Log: `/tmp/stage/review-evidence/cargo-test-gyre-domain-analytics.log`.
- `cargo test -p gyre-adapters analytics` → **16 passed** (incl. `analytics_query_filtered_all_params` with a cross-cutting seed so dropping any single predicate changes the asserted result sets, `analytics_event_name_prefix_is_literal_case_sensitive`, `analytics_count_grouped_over_full_filtered_set`, `analytics_count_grouped_by_day`). Log: `/tmp/stage/review-evidence/cargo-test-gyre-adapters-analytics.log`.
- `cargo check -p gyre-server --tests` → **exit 0** (6m42s; full server test tree type-checks, including the merge of task-130's `caller_tenant` signature change and this branch's analytics tests). Log: `/tmp/stage/review-evidence/cargo-check-gyre-server-tests.log`.
- All 18 mechanical invariant scripts → **exit 0** each (`check-arch`, `check-migration-versions`, `check-inert-enforcement`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-abac-route-registry`, `check-mcp-write-tools`, `check-mem-port-contracts`, `check-byte-slice-truncation`, `check-dead-message-kinds`, `check-migration-sql-portability`, `check-forged-scope-fields`, `check-lossy-secret-conversion`, `check-unbounded-external-http`, `check-forwarded-header-trust`, `check-in-memory-state-stores`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`). Logs under `/tmp/stage/review-evidence/check-*.log`. No new exemption-file entries (script diffs are comment-only).
- Diff inspection of every emission site (see below); grep sweep for all 12 event names across `crates/gyre-server/src`.

## What was verified

**§Event Schema** — `gyre-domain/src/analytics.rs` `AnalyticsEvent` carries all 9 spec fields (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`, `repo_id`, `properties`, `timestamp`); `with_scope()` populates the four scope ids. All three adapters persist them: SQLite + Postgres via new migration `2026-10-08-000063_analytics_scope_columns` (unique sequence number; portable SQL — passes `check-migration-sql-portability`), mem in-code. Spec types ids as `Option<Id>`; adapters store text and the coverage note records that mapping explicitly — a faithful, documented representation, not a reinterpretation.

**§Auto-Emitted Events** — all 12 events emit at real trigger points, each inspected in production (non-test) code:

- `task.status_changed` (`old_status`/`new_status`/`task_id`/`assigned_to`): centralized `record_task_status_changed`, called from the HTTP transition endpoint, MCP `update_task`, spawn/completion auto-advance, admin force-kill, repo-archive and spec-reject cancels, stale-abort, ralph-loop re-open/ceiling/terminal paths, and the signal-chain scheduler — 22 call sites.
- `mr.merged` (`mr_id`/`repo_id`/`gate_count`/`queue_wait_secs`): HTTP transition endpoint AND queue path (`emit_mr_merged`, deferred past atomic-group rollback so no phantom merged events).
- `mr.closed` (`mr_id`/`repo_id`/`reason`): transition endpoint (`closed_without_merge`), spec-reject (`spec_rejected`), repo-archive (`repo_archived`), ralph-loop gate-failure close.
- `agent.spawned` (`task_id`/`compute_target`/`persona`): worker spawn reports the *bound* persona (not a hardcoded label) and the launch's resolved compute target; orchestrator spawn reports the actually-launched compute target with honest null `task_id`; ralph-loop re-spawn marked `respawn: true`.
- `agent.completed` (`task_id`/`duration_secs`): HTTP complete handler and MCP `gyre_agent_complete` (both emit; test at mcp.rs:6128 pins the MCP path against the old no-event behavior).
- `agent.failed` (`task_id`/`reason`): fail/stop endpoints, MCP, admin kill, repo-archive, spec-reject, stale-abort (new `stale_agents.rs` module), ralph-loop ceiling/terminal re-spawn failure, worker failed-launch teardown, orchestrator process-death monitor.
- `merge_queue.processed` (`mr_id`/`outcome`/`wait_secs`): merged path plus every terminal path funneled through `fail_queue_entry`/`emit_queue_processed_terminal` (outcome `failed`), circuit-breaker Cancelled (`skipped`).
- `gate.failed`/`gate.passed` (`gate_id`/`gate_type`/`mr_id`/`output_snippet`|`duration_secs`): pre-merge `run_gate` AND post-merge `run_post_merge_gate_commands` (revert/recovery runs included); no `gate.passed` on a failing gate (negative test).
- `spec.approved` (`spec_path`/`approver_type`/`approval_mode`): specs.rs approval handler, approver identity split agent/human into the schema's `agent_id`/`user_id` fields.
- `budget.warning` (`workspace_id`/`metric`/`threshold_pct`): `emit_budget_warning` at 80% threshold, invoked from spawn-budget checks for all three metrics (agents/tokens/cost); negative test below threshold.
- `search.query` (`query_length`/`entity_types`/`result_count`/`duration_ms`): HTTP search handler and MCP `gyre_search` via the shared `record_search_query`; identity caller-bound, workspace scope validated against the caller's tenant before attribution (cross-tenant forgery is Forbidden, pinned by test).

**§Query API / §Query Parameters** — `GET /api/v1/analytics/events` supports `event_name` (exact or trailing-`*` prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 with full TZ/fractional parsing plus legacy unix seconds; date-only `until` extends to end-of-day), `limit` (default 100, capped 10 000 — exactly the spec numbers), and `group_by` (`event_name`/`agent_id`/`workspace_id`/`day`, unsupported values rejected). `GET /daily` supports the spec's `?event_name=&days=30` form. Prefix matching is literal and case-sensitive on **all three** adapters (mem `starts_with`, SQLite `substr(...) = ...` under BINARY collation, Postgres wildcard-escaped LIKE) with regression tests pinning `MR.*`/`mr.m_rged*` match nothing. `group_by` aggregates server-side over the full filtered set (port-level GROUP BY); `limit` bounds returned groups, never the aggregation input — grouped totals stay consistent with `/count` (regression test `analytics_count_grouped_over_full_filtered_set`). Day keys are UTC calendar days on all three adapters.

**Anti-forgery** — `POST /analytics/events` ignores request-body `agent_id`/`user_id` and binds identity to the verified caller (agent JWT → agent, user/API key → user, impersonation → impersonator); `workspace_id`/`repo_id` are validated within the caller's tenant, rejecting forged scope attribution. Auto-emitted events derive scope from the persisted entities, never caller input.

## Test-inflation check

None found. Per-event tests drive the real handlers/router (or the real internal function) and then query the analytics repository for the recorded event, asserting event count and each spec property; negative tests exist (below-threshold budget, failed gate emits no `gate.passed`, cross-tenant scope rejected, unscoped filters return empty). The adapter filter test uses a cross-cutting seed (agents span workspaces, workspaces span repos, users span workspaces) so removing any one predicate changes the asserted results — it is not self-confirming.

## Non-blocking observations (recorded, not findings)

- `GET /analytics/events` performs no tenant scoping of results (any authenticated caller with analytics read can query across tenants by filter). This is **pre-existing at base** — the base endpoint already returned all events with only `event_name`/`since`/`limit` and no tenant constraint — and the task explicitly forbids new endpoints/behavior beyond adding filters. The candidate strictly strengthened the surface (caller-bound identity on write, tenant-validated scope attribution). Flagged for a future platform task, not this one.
- The shared migrations dir contains pre-existing duplicate 6-digit sequence numbers from earlier parallel-branch merges (e.g. two dated `000061` dirs); `check-migration-versions.sh` exits 0 on this tree and the candidate's new `000063` is unique. Pre-existing debt, unchanged by this candidate.
- Working-tree note: the stage setup left `specs/tasks/task-146.md` checked out at its base content (progress `not-started`); the committed candidate `91f22ac1` contains the correct `ready-for-review` frontmatter with the full `commits:` list. No production file was locally modified.

## Server tests the independent verifier must execute

(deferred from this review per the 4 GiB build budget; `cargo check -p gyre-server --tests` already passes)

- `cargo test -p gyre-server analytics`
- `cargo test -p gyre-server gate_executor`
- `cargo test -p gyre-server orchestrator`
- `cargo test -p gyre-server ralph_loop`
- `cargo test -p gyre-server mcp`
- `cargo test -p gyre-server merge_processor`
- `cargo test -p gyre-server stale_agents`
- `cargo test -p gyre-server --test api_integration`
