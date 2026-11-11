# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate: `1d0ae5121a70a5d7e469564f157ebe83d473edb0` (branch `pipeline/task-146/e218d5eea66b4089bcc251278570e760-1`), assigned base `cfeefbda`.
Verdict: **approved** — no blocking findings. Independent evidence below; server test execution deferred to the verifier with an explicit list (executor memory constraint), evidence in `/tmp/stage/review-evidence/`.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `/tmp/stage/bin/cargo test -p gyre-domain analytics` → **3 passed**, incl. `analytics_event_matches_spec_schema` (probe1).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib analytics` → **13 passed**, incl. `analytics_query_filtered_all_params` (probe2).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean, 5m13s (probe3).
- Mechanical invariant checks, all exit 0 (probe4 + logs): `check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-fabricated-scope-defaults.sh`, `check-forged-scope-fields.sh`, `check-abac-route-registry.sh`, `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`, `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`.

## What was verified (source inspection of every emission site + executed probes)

- **Event schema (AC1):** `AnalyticsEvent` (gyre-domain/src/analytics.rs) now carries all 9 spec fields; `with_scope` populates user/session/workspace/repo. The domain test pins the shape incl. a serde round-trip (executed, passed). Id-typed fields are stored as `Option<String>` matching the pre-existing `agent_id` pattern and the TEXT storage columns — documented in the coverage matrix; semantically equivalent, not a hollow implementation.
- **All 12 auto-emitted events (AC2/AC3), each with spec-required properties, at real trigger points:**
  - `task.status_changed` (old/new/task_id/assigned_to): shared helper `record_task_status_changed` called from 8 production paths — HTTP status endpoint (tasks.rs), MCP `update_task`, spawn auto-assign, completion auto-advance, admin force-kill, stale-abort, spec-reject cancel, repo-archive cancel.
  - `mr.merged` (mr_id/repo_id/gate_count/queue_wait_secs): HTTP transition (merge_requests.rs, gate_count from real `gate_results`, queue_wait from real queue entry) **and** the queue-merge path (`emit_mr_merged` in merge_processor.rs) — both paths emit; per-path test for the queue path exists (`queue_merge_records_mr_merged_analytics_event`).
  - `mr.closed` (mr_id/repo_id/reason): HTTP close, repo-archive close, spec-reject close.
  - `agent.spawned` (agent_id/task_id/compute_target/persona): worker spawn (persona `default-worker`, real compute-target label) and orchestrator spawn (tier persona; `task_id` honestly null for non-task-bound orchestrators rather than fabricated).
  - `agent.completed` (agent_id/task_id/duration_secs): HTTP complete and MCP `gyre_agent_complete`.
  - `agent.failed` (agent_id/task_id/reason): fail endpoint, kill endpoint, admin force-kill, stale-abort, spec-reject stop, repo-archive stop, compose-session teardown.
  - `merge_queue.processed` (mr_id/outcome/wait_secs): merged, every terminal Failed path (funneled through `fail_queue_entry`/`emit_queue_processed_terminal` so status and event cannot diverge), circuit-breaker Cancelled and operator cancel as outcome `skipped`.
  - `gate.failed`/`gate.passed` (gate_id/gate_type/mr_id/output_snippet|duration_secs): single funnel `run_gate` in gate_executor.rs — all gate types (command, agent review/validation, trace capture) execute through it; `output_snippet` uses char-boundary-safe `truncate_bytes` (UTF-8 test included), and the event's workspace/repo scope is resolved from the real MR (skip on unresolvable, no "default" fabrication — the fixed site the lowered exemption count reflects).
  - `spec.approved` (spec_path/approver_type/approval_mode): specs.rs approval handler, approver user/agent derived from the approval event, not caller-supplied.
  - `budget.warning` (workspace_id/metric/threshold_pct): wired into `check_spawn_budget` (the spawn enforcement path — agents/tokens/cost metrics) and cost recording; threshold 80%; negative-control test proves no emission below threshold.
  - `search.query` (query_length/entity_types/result_count/duration_ms): search handler, real measured `duration_ms`, distinct result types; empty-query short-circuit emits nothing (negative-control test).
- **Query API / Query Parameters (AC4):** no new routes (complies with "Do NOT create new endpoints"); `GET /api/v1/analytics/events` supports `event_name` (exact + trailing-`*` prefix via LIKE on both SQLite and Postgres), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs, inclusive), `limit` (default 100, cap 10_000), `group_by` ∈ {event_name, agent_id, workspace_id, day} with unsupported fields rejected; `GET /daily` supports the spec's `?event_name=&days=30` form. Implemented on SQLite, Postgres, and mem adapters with a parity test. The adapter test `analytics_query_filtered_all_params` is genuinely discriminating: cross-cutting seed (agents span workspaces, workspaces span repos, users span workspaces) with an individual assertion per predicate — executed, passed.
- **Coverage matrix honesty:** sections 1-5 marked `implemented` with accurate evidence; sections 6-12 (Decision API) remain `task-assigned` to task-147 — no fake completion. Canonical spec `specs/system/analytics.md` untouched.
- **Out-of-scope-tolerance:** the diff vs base includes rebased-in work from merged bases (task-091 et al., per the branch's rebase history); the task-146-attributed product commits were reviewed here. The `api_integration.rs` addition is a real tenant-containment test for the orchestrator spawn (403 cross-tenant, positive control in-tenant) from a prior review-repair round. Rate-limit changes are test-determinism fixes (frozen clock) that strengthen, not inflate, tests.

## Test-inflation check

None found. Per-event tests drive real handlers through the actual router (`app.oneshot(...)`) or real queue/processor functions and assert exact property values; negative controls exist (budget below threshold, empty search query, exact-vs-prefix event-name discrimination). The adapter filter test fails if any single predicate is dropped. The one limitation is procedural, not evidential: the server lib tests could not be executed in this review stage (executor 4 GiB constraint on gyre-server test compilation) — the exact list for the verifier is in `/tmp/stage/review-evidence/verifier-must-run.txt` (per-event tests, HTTP query-param tests, mem parity test, plus `--test api_integration`).

## Non-blocking observations (recorded for a future hardening owner, no action required for this task)

- `POST /api/v1/analytics/events` persists caller-supplied `user_id`/`workspace_id`/`repo_id` scope fields without caller-scope validation. This extends the pre-existing pattern (`agent_id` was already caller-supplied at base) to the new schema fields, is consistent with the spec's design of the endpoint as client telemetry ("analytics is not an audit trail"), and escapes the repo's `check-forged-scope-fields.sh` by design (that check is scoped to handlers extracting `AuthenticatedAgent`; this handler relies on middleware ABAC). A caller can attribute telemetry to another workspace, polluting that workspace's filtered analytics. If analytics ever feeds authorization or compliance decisions, derive-or-validate should be added; the Decision API (task-147) will read these events, so that task's review should re-examine this.
