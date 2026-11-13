# Review: TASK-146 — Complete analytics event schema and auto-emitted events coverage

**Reviewer:** Independent review agent
**Date:** 2026-11-13
**Candidate:** `7a597203d1407bce61b8f8f92ae0f94efffcc5c1` (base `f764bc005c5ff8f442f58556a655fb9902049aa9`)
**Verdict:** approved

---

## Scope reviewed

All five assigned coverage sections (`analytics.md` §Purpose, §Event Schema,
§Auto-Emitted Events, §Query API, §Query Parameters) against the candidate diff
(base → candidate). Server test *execution* deferred to the independent
verifier per the 4 GiB budget rule; type-checking of lib + all test cfg was
performed here (see Evidence).

## Findings

None blocking.

- **Event schema (§Event Schema)** — `AnalyticsEvent` (gyre-domain/src/analytics.rs)
  carries all 9 spec fields. Id-typed scope fields are stored as `Option<String>`
  (wire-compatible with `Option<Id>`, which is a `String` newtype; documented in
  the coverage row). `analytics_event_matches_spec_schema` unit test passes.
- **Auto-emitted events (§Auto-Emitted Events)** — all 12 events emit at real
  production trigger points, each with all spec-required properties:
  - `task.status_changed` (api/analytics.rs `record_task_status_changed`):
    HTTP task status endpoint, MCP `update_task`, spawn/completion auto-advance,
    admin force-kill, stale-abort, spec-reject cancel, repo-archive cancel,
    ralph-loop re-open — 11 call sites.
  - `mr.merged`: HTTP MR transition endpoint + merge-queue merge path
    (`emit_mr_merged`), with `gate_count` (real `gate_results` lookup) and
    `queue_wait_secs` (enqueue→merge delta).
  - `mr.closed`: MR transition endpoint, ralph-loop gate-failure close,
    spec-reject close, repo-archive close — `reason` is per-path
    (`closed_without_merge`/`gate_failure`/`spec_rejected`/repo-archive), not a
    fabricated constant.
  - `agent.spawned`: worker spawn (bound `persona` slug + real
    `compute_target_label` from the launch outcome) and orchestrator spawn
    (task_id honestly null for non-task-bound orchestrators).
  - `agent.completed`: HTTP complete endpoint and MCP `gyre_agent_complete`.
  - `agent.failed`: fail/stop endpoints, admin kill, stale-abort, spec-reject,
    repo-archive, ralph-loop iteration ceiling and re-spawn failure, budget
    kill, compose teardown, orchestrator death monitor.
  - `merge_queue.processed`: every terminal queue path (merged, gate failure,
    missing MR/repo, spec policy, breaking-change policy, spec-link gates,
    constraint enforcement, attestation-ABAC denial, dependency closed,
    circuit-breaker cancel → outcome "skipped").
  - `gate.failed`/`gate.passed`: gate_executor after each gate run, with
    `output_snippet` (char-boundary-safe truncation) / `duration_secs`.
  - `spec.approved`: specs.rs approval handler, on transition into Approved.
  - `budget.warning`: spawn-budget check and usage-recording paths, 80%
    threshold, metric tokens/cost/agents, workspace-scoped.
  - `search.query`: HTTP search endpoint and MCP `gyre_search`, identity bound
    to the verified caller, workspace scope validated against caller's tenant;
    empty query (no search executed) correctly emits nothing.
- **Query API / Query Parameters** — `GET /api/v1/analytics/events` supports
  `event_name` (exact + trailing-`*` prefix), `agent_id`, `user_id`,
  `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs), `limit`
  (default 100, max 10_000), `group_by` ∈ {event_name, agent_id, workspace_id,
  day} (unsupported → 400). Semantics identical across mem, SQLite, and
  Postgres adapters (parameterized Diesel). `group_by` aggregates the full
  filtered set server-side; `limit` bounds returned groups. No new endpoints —
  ABAC registry and middleware untouched (task instruction respected).
- **Anti-forgery** — `record_event` ignores caller-supplied identity fields and
  validates workspace/repo scope against the caller's tenant; mismatch rejects
  the write. Tests cover both.
- **Migrations** — `2026-10-08-000063_analytics_scope_columns` uses the next
  unused sequence number, portable SQL (both SQLite and PG), with indexes;
  version and portability checks pass.
- **Tests are real** — per-event tests trigger the actual action (HTTP request
  on the real router, or real gate/queue execution) and then assert the
  recorded event's properties against the wired analytics repository; a
  mutation probe (disabling the `workspace_id` SQL filter) makes
  `analytics_query_filtered_all_params` fail, proving the filter tests are not
  self-confirming.
- **Coverage matrix honesty** — analytics.md rows 1–5 marked `implemented`
  with accurate notes; Decision-API rows 6–12 correctly left `task-assigned`
  (task-147), no side-effect closures. scripts/ + exemption-file changes in the
  diff are comment-only (frozen counts unchanged); all 19 mechanical invariant
  scripts pass on the candidate tree; task-commit attribution passes against
  the committed candidate.

## Non-blocking observations

- The stage setup left an unstaged working-tree edit to `specs/tasks/task-146.md`
  (emptied `commits:`, progress reset to `not-started`). The committed candidate
  is self-consistent; this is checkpoint bookkeeping, not a source defect.
- `Option<String>` vs the spec's `Option<Id>` on scope fields is a documented,
  wire-compatible storage choice, not a contradiction.

## Evidence

`/tmp/stage/review-evidence/` — probes.md (summary), domain-test.log (exit 0),
adapters-test.log (exit 0, 15 tests), server-check.log (cargo check -p
gyre-server, exit 0), server-check-tests.log (cargo check --lib --tests,
exit 0), invariant-scripts.log (19/19 pass), attr-check.log /
attr-candidate-tree.log, mutation-probe.log.

## Verifier must run (server test execution deferred per budget rule)

- `cargo test -p gyre-server --lib`
- `cargo test -p gyre-server --test api_integration`
- `cargo test --all` (workspace) and `cd web && npm ci && npm test` (no web/
  source changed by this task)
