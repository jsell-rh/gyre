# Review: task-146 — Complete analytics event schema and auto-emitted events coverage

**Candidate:** `edefa99fb57d65692713a3020d81e484168fc140`
**Base:** `a218b9565608f4bf0dc61477688013b0264c2edc`
**Spec:** `specs/system/analytics.md` §Purpose, §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters
**Verdict:** NOT APPROVED — 1 blocking finding
**Review round:** after repair rounds 24b2d851a (migration renumber), f301cff2c (fmt/clippy), ad7d3e33a (caller-bound identity), e9e597a07 (terminal-path events + search scope), 5fb9e9b83 (persona attribution)

## What was verified (independently)

### Event Schema
`AnalyticsEvent` (crates/gyre-domain/src/analytics.rs) carries all 9 spec fields;
`with_scope` populates user/session/workspace/repo. Id-typed fields are
`Option<String>` matching the storage layer's TEXT columns (documented in the
coverage matrix); the serde shape is identical to the spec's. Round-trip test
`analytics_event_matches_spec_schema` run locally: **pass**.

### Auto-Emitted Events — all 12 at real trigger points with spec properties
- `task.status_changed` — HTTP status endpoint (enriched with
  `old_status`/`assigned_to` this round), MCP `update_task`, spawn/completion
  auto-advance, admin force-kill block, stale-abort block, spec-reject cancel,
  repo-archive cancel, ralph-loop re-open and iteration-ceiling paths.
- `mr.merged` — queue merge (`emit_mr_merged`) + HTTP transition; both carry
  `mr_id`, `repo_id`, `gate_count` (real gate-result count), `queue_wait_secs`.
- `mr.closed` — transition endpoint, repo archival, spec-reject, ralph-loop
  gate-failure/ceiling close; carries `mr_id`, `repo_id`, `reason`.
- `agent.spawned` — worker spawn (`compute_target` = resolved launch label,
  `persona` = bound persona slug), ralph-loop re-spawn (persona resolved from
  `agent.persona_id`, fail-closed on non-Approved), orchestrator spawn. See
  finding F1 for the orchestrator `compute_target` defect.
- `agent.completed` — HTTP complete endpoint and MCP `gyre_agent_complete`;
  `agent_id` (event field), `task_id`, `duration_secs`.
- `agent.failed` — shared `record_agent_failed` helper on fail/stop/kill
  endpoints, admin force-kill, agent status endpoint, stale-abort, compose
  teardown, orchestrator process-death monitor, worker failed-launch teardown,
  ralph-loop ceiling/terminal respawn failure; `reason` names the cause.
- `merge_queue.processed` — merged/failed/skipped outcomes via
  `emit_queue_processed_terminal`/`fail_queue_entry` chokepoints covering every
  terminal path incl. operator cancellation and circuit-breaker; `wait_secs`
  from `enqueued_at`.
- `gate.failed`/`gate.passed` — single chokepoint in `gate_executor::run_gate`
  after the gate-type match; `output_snippet` byte-safe-truncated (512),
  `duration_secs` measured; `gate.passed` not emitted for failing gates.
- `spec.approved` — approval handler; `spec_path`, `approver_type`,
  `approval_mode`; approver identity derived from verified JWT claims
  (agent/human), never the request body.
- `budget.warning` — 80% threshold on agents/tokens metrics from the budget
  check and usage-recording paths; `workspace_id`, `metric`, `threshold_pct`.
- `search.query` — search handler after a real search; `query_length`
  (char count), `entity_types` (deduped result types), `result_count`,
  `duration_ms`; identity bound to the verified caller, scope validated.

### Query API / Query Parameters
Routes unchanged (no new endpoints, per task instruction). `query_filtered`
implemented on **SQLite, Postgres, and mem** with the full spec filter set:
`event_name` (exact or trailing-`*` prefix), `agent_id`, `user_id`,
`workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix seconds),
`limit` (default 100, cap 10_000), `group_by`
(`event_name`/`agent_id`/`workspace_id`/`day`, unsupported fields rejected).
`record_event` ignores body identity fields (binds to the verified caller) and
validates workspace/repo scope against the caller's tenant (Forbidden on
mismatch, NotFound on unknown). Ran locally:
`cargo test -p gyre-adapters --lib sqlite::analytics` — **13 passed**,
including the cross-cutting `analytics_query_filtered_all_params`
(each scope predicate individually mutation-killing).

### Security/scope posture
Approver and search identities caller-bound; `validate_*_scope` results are
propagated with `?`; no `unwrap_or_default()` on ref resolution; no lossy
secret conversion; migration `2026-10-08-000063_analytics_scope_columns` is
portable SQLite+PG SQL; exemption-file edits are comment-only (no new
entries). All 16 mechanical invariant checks pass (logs in
/tmp/stage/review-evidence).

### Build
`SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` — **clean**.
No `web/` changes in the diff (generated bundle ownership: verifier).

## Findings

### F1 (blocking) — orchestrator `agent.spawned` hardcodes `compute_target: "local"`
`crates/gyre-server/src/api/orchestrator.rs`, `spawn_orchestrator`: the event
is emitted with the literal `"local"` before `launch_orchestrator_process`
resolves the actual target (workspace assignment → tenant default → local).
With a configured workspace/tenant compute target (supported and tested — see
`launch_with_command` in the same file's tests) the recorded value is wrong.
The worker and re-spawn paths report the resolved `launch.compute_target_label`;
only this path hardcodes. The test
`orchestrator_spawn_emits_agent_spawned_analytics_event` asserts the literal,
so it passes with the lookup disabled — the hardcoded-value-standing-in-for-a-
looked-up-one class the task goal bans, and the same class fixed for this
event's `persona` in round 5fb9e9b83. Fix: resolve the label (or carry it on
`LaunchOutcome`) and emit the resolved value.

## Non-blocking observations
- `budget.warning` re-fires on each budget check while usage ≥ 80%; each event
  is truthful, spec does not require once-per-crossing dedup.
- A prefix filter containing `%`/`_` acts as a LIKE wildcard (broader match
  only; parameterized, no injection).
- Pre-existing at base, not introduced here: sqlite analytics rows carry the
  literal `tenant_id: "default"` (grandfathered pattern across adapters).

## Verifier must run (deferred from this sandbox — 4 GiB budget)
- `cargo test -p gyre-server --lib` (per-event analytics tests across
  api/{analytics,spawn,budget,search,specs,merge_requests,orchestrator,tasks,
  admin,compose,repos,merge_queue,agents}.rs, mcp.rs, merge_processor.rs,
  gate_executor.rs, ralph_loop.rs, stale_agents.rs, mem.rs, middleware.rs,
  rate_limit.rs)
- `cargo test -p gyre-server --test api_integration` (`analytics_record_and_query`)
- `cargo test -p gyre-adapters --lib`, `cargo test -p gyre-domain`
- CI fmt/clippy on the merged tree
