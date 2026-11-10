# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Comparison base: `b5a872087899e8bc4a18092ab56990a26e9ca9af` → candidate `2a9f4f0d4e7b3425fa10504155671d2ae4b41ce8`
(merge commit `72da380b3` + process commit `2a9f4f0d4`).

Verdict: **not approved** — one blocking merge regression; the task-146 surface itself is real.

## Task-146 contract (verified, passes)

- `AnalyticsEvent` carries all 9 spec fields; `analytics_event_matches_spec_schema`
  round-trip test pins the shape (gyre-domain full suite: 444+8 passed).
- All 12 spec-required auto-emitted events emit at real production trigger points with
  all spec-required properties. Multi-path coverage is genuine: `merge_queue.processed`
  on every terminal queue transition (via `fail_queue_entry` /
  `emit_queue_processed_terminal`, incl. operator cancel as `skipped`),
  `task.status_changed` via the shared `record_task_status_changed` helper on HTTP,
  MCP, auto-advance, admin kill, stale-abort, spec-reject and repo-archive paths,
  `agent.failed` on all kill paths incl. compose teardown, `agent.completed` on both
  the HTTP and MCP completion paths. Per-event regression tests assert presence and
  properties through `state.analytics.query(...)`.
- Query API filter set complete (`event_name` exact/prefix, `agent_id`, `user_id`,
  `workspace_id`, `repo_id`, `since`, `until`, `limit`, `group_by`) on SQLite, Postgres
  and mem adapters; the mem adapter implements real filtering (handler tests are not
  vacuous); adapter test `analytics_query_filtered_all_params` exercises every param.
- Focused suites on the candidate all pass (analytics 20, merge_processor 91,
  gate_executor 26, compose 8, merge_requests 35, merge_queue 9, budget 11, search 2,
  spawn 51, orchestrator 12, stale_agents 4, middleware 26, rate_limit 19, specs 18,
  mcp 5, tasks 7, admin 36, repos 18, domain full, adapters analytics 13) and the full
  `cargo test -p gyre-server --lib` run passes: **1658 passed, 0 failed**.
- Offline gates: every AGENTS.md-enforced `scripts/check-*.sh` exits 0; non-enforced
  heuristic scripts that fail do so identically on base (per-script output diff in a
  detached base worktree). Exemption files only shrank. Migration
  `2026-10-08-000059_analytics_scope_columns` uses the next unused sequence and
  portable SQL. web/dist byte-identical to base.

## Blocking finding

**Merge regression: `orchestrator_spawn_denied_across_tenants` fails** —
`cargo test -p gyre-server --test api_integration` exits 101 (66 passed, 1 failed);
single-test rerun is deterministic.

- Symptom: `crates/gyre-server/tests/api_integration.rs:1680` — cross-tenant
  orchestrator spawn must be 403, got **404**.
- Cause: merge `72da380b3` adopted base's `is_system_principal()` tenant bypass in
  `spawn_workspace_orchestrator` (`crates/gyre-server/src/api/orchestrator.rs:643`)
  alongside base's fail-closed `resolve_orchestrator_persona` (line 601). The
  integration global token authenticates as the system principal, so the tenant check
  is bypassed; the spawn then fails persona resolution with `NotFound` (the test's mem
  state has no approved `workspace-orchestrator` persona) → 404. The branch's own test
  (task-093 F2 / task-146 repair lineage) was written for the pre-merge strict check
  and was not reconciled with the adopted base semantics.
- Proof of regression: the identical test **passes** on branch parent `a34d06c06`
  (detached worktree, same probe) and fails deterministically on the candidate.
- The executor's rebase-round notes state `cargo test` could not run in that sandbox
  (crates.io egress blocked there), so the regression shipped uncaught. The
  deterministic verification stage will fail on it exactly like the earlier
  rate-limit race round.
- Repair direction (executor's choice): either reconcile the test with the merged
  semantics (seed an approved `workspace-orchestrator` persona, and assert the
  system-principal bypass explicitly with a non-system principal still getting 403),
  or scope the bypass — but the shipped tree must have its own integration suite
  green.

## Non-blocking observations

- `persona: "default-worker"` on the worker `agent.spawned` emission is a documented
  constant mapping (agent-runtime.md §Bootstrap); orchestrator spawns report their
  tier persona. Acceptable reading of the spec property.
- The merge adds +1 `check-mcp-wrapper-parity` heuristic finding (hand-built response
  in `mcp.rs handle_agent_complete`, where `agent.completed` was added) and +16
  `let _ = state.analytics.record(&ev)` telemetry discards — both in scripts that
  already exit 1 on base, following pre-existing codebase patterns. The branch also
  fixed one pre-existing cli-spec-parity violation (`assigned_to` param).
- SQLite/Postgres prefix `event_name` filtering uses SQL LIKE without escaping `%`/`_`
  while mem uses `starts_with`; divergence only for event names containing LIKE
  wildcards (none exist). Cosmetic.

Evidence: `/tmp/stage/review-evidence/` (per-suite logs, gate outputs with base
comparison, single-test and parent-worktree runs proving the regression).
