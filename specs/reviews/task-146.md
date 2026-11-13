# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters.
Candidate under review: `96e20edf` (base `276918d`; 93 commits base→candidate including
rebase merges of main — task-128/139/119/134/103/104/168/193 reconciliation — and 21
attributed task-146 commits recorded in the task frontmatter).
Verdict: **approved** — no findings. Prior review rounds' repairs (group_by full-set
aggregation, persona/compute-target attribution, MCP search.query recording, scope
validation on search attribution) are all present in the candidate and re-verified here.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain` → 504 passed, incl. `analytics_event_matches_spec_schema`
  (serde round-trip pinning all 9 spec fields).
- `cargo test -p gyre-adapters analytics` → 15 passed against real SQLite storage, incl.
  `analytics_query_filtered_all_params` (cross-cutting seed: no scope predicate is
  redundant with another, so dropping any one changes the asserted results) and
  `analytics_count_grouped_over_full_filtered_set`.
- `cargo check -p gyre-server` → clean (server test compilation deferred to the
  verifier per the review-stage memory budget; per-event server tests verified by
  inspection — they drive the real router via `oneshot` and then query the store).
- Mechanical invariants all pass: arch, ABAC route registry, MCP write-tool gating,
  dead message kinds, fabricated-scope defaults, scope-literal defaults, inert
  enforcement, lossy-secret conversion, byte-slice truncation, migration versions
  (000063 unique, next unused after base's duplicate 000062), migration SQL
  portability, mem port contracts.

## What was verified

- **Event Schema**: `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries every spec
  field — id, event_name, agent_id, user_id, session_id, workspace_id, repo_id,
  properties, timestamp. Id-typed fields are stored as `Option<String>` matching the
  storage layer's TEXT columns; `with_scope` populates the scope ids. Migration
  `2026-10-08-000063_analytics_scope_columns` adds the four scope columns + indexes,
  portable across SQLite/Postgres, with a correct down.sql.
- **All 12 auto-emitted events at real trigger points with all spec properties**:
  - `task.status_changed` (old/new/task_id/assigned_to) — HTTP status endpoint, MCP
    `gyre_update_task`, spawn/completion auto-advance, admin force-kill, stale-abort,
    spec-reject, repo-archive, ralph-loop re-opens and ceiling/terminal Blocks.
  - `mr.merged` (mr_id/repo_id/gate_count/queue_wait_secs) — HTTP transition endpoint
    and both queue merge paths (single + atomic group, deferred past rollback).
  - `mr.closed` (mr_id/repo_id/reason) — transition endpoint, repo-archive/spec-reject,
    ralph-loop gate-failure close.
  - `agent.spawned` (agent_id/task_id/compute_target/persona) — worker path reports
    the launched compute target and the bound persona; orchestrator path reports the
    resolved workspace compute target (not a hardcoded "local") and an honest null
    task_id.
  - `agent.completed` (agent_id/task_id/duration_secs) — HTTP complete endpoint and
    MCP `gyre_agent_complete` (the autonomous path).
  - `agent.failed` (agent_id/task_id/reason) — fail/stop/kill endpoints, MCP tools,
    compose teardown, stale-abort, repo-archive, ralph-loop ceiling/terminal, worker
    failed-launch teardown, orchestrator process-death monitor, agent status endpoint.
  - `merge_queue.processed` (mr_id/outcome/wait_secs) — merged outcome on both queue
    paths plus every terminal Failed path through `fail_queue_entry`, and the
    circuit-breaker Cancelled as outcome "skipped".
  - `gate.failed` (gate_id/gate_type/mr_id/output_snippet, char-boundary-safe 512-byte
    truncation) and `gate.passed` (…/duration_secs) — gate_executor evaluation loop.
  - `spec.approved` (spec_path/approver_type/approval_mode) — specs.rs approval handler.
  - `budget.warning` (workspace_id/metric/threshold_pct) — spawn-budget and usage
    recording paths, 80% threshold, not emitted below threshold (negative test).
  - `search.query` (query_length/entity_types/result_count/duration_ms) — HTTP search
    endpoint and MCP `gyre_search`, both through one helper.
- **Query API/Parameters**: every spec parameter implemented — event_name (exact or
  trailing-`*` prefix via LIKE on SQLite and Postgres), agent_id, user_id (extra),
  workspace_id, repo_id, since/until (ISO8601 or unix secs, inclusive; date-only
  upper bounds extend to end-of-day), limit (default 100, cap 10_000), group_by
  (event_name/agent_id/workspace_id/day; unsupported values → 400). group_by aggregates
  server-side over the FULL filtered set via port-level `count_grouped` implemented on
  all three adapters (SQL GROUP BY on SQLite/Postgres with the WHERE clause mirroring
  `query_filtered`; mem uses a shared `analytics_event_matches_filter` predicate so the
  grouped path cannot drift); `limit` bounds returned groups. `/daily` supports the
  spec's `?event_name=&days=30` form with until defaulting to now.
- **Scope safety**: `POST /analytics/events` ignores client-supplied agent_id/user_id
  and binds identity to the verified caller (agent JWT → agent, user/API key → user,
  impersonation → impersonator), and validates workspace/repo attribution against the
  caller's tenant (Forbidden/NotFound on mismatch) — the same anti-forgery rule as
  audit events. `search.query`'s workspace attribution is validated the same way on
  both the HTTP and MCP paths before the event is recorded. Auto-emit sites attribute
  scope from server-side entities (task/workspace/repo/MR lookups), never caller input.
- **No fake completion**: coverage matrix marks exactly sections 1–5 implemented with
  accurate notes; sections 6–12 (Decision API) remain task-assigned to task-147.
  Exemption-file edits only lower counts / update comments (no new exemptions).

## Tests the verifier must run

- `cargo test -p gyre-server` — per-event integration tests live in
  api/analytics.rs, api/spawn.rs, api/budget.rs, api/search.rs, api/specs.rs,
  api/merge_requests.rs, api/tasks.rs, api/agents.rs, merge_processor.rs,
  gate_executor.rs, ralph_loop.rs, mcp.rs, mem.rs, stale_agents.rs.
- `cargo test -p gyre-adapters` (full suite beyond the analytics filter run here).
- `cargo clippy --all` per repo CI.
