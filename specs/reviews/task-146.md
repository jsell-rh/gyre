# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema / §Auto-Emitted Events / §Query API / §Query Parameters (§Purpose is context).
Comparison base: `cba416b46a73d3568f745d7b4eae49a0700a90a6` → candidate `c5e9ffd22f50380290113a6ac91080fa889bee10`.
Diff: 38 files, +5010/−346. No new routes (api/mod.rs untouched), no new exemption entries.

Verdict: **complete** (approved).

## Round history

Prior rounds' durable findings all addressed in the candidate:
- F1 (forged-scope attribution in `record_event`, review checkpoint fb26ebd6) — repaired by `ad7d3e33a` and verified this round (below).
- Rebase conflicts (spawn.rs, SUMMARY.md) — resolved; the merge-processor/orchestrator overlap with task-134/task-093 lineage reconciled cleanly in the final tree.
- Earlier verification repairs (terminal-path emissions `8f0ee190`, deterministic rate-limit test `d7641ddd`, adapter filter assertions `897b5565`, migration renumbering `29d73e81`/`17face90`/`6f469a6b`, tenant containment `3bcd6f52`) — all present and inspected.

## Acceptance criteria vs. evidence

1. **Struct matches spec schema** — all 9 fields on `AnalyticsEvent` (gyre-domain/src/analytics.rs); `with_scope` populates user/session/workspace/repo; `analytics_event_matches_spec_schema` serde round-trip test. Id-typed optional fields are `Option<String>`, the pre-existing storage convention (agent_id was `Option<String>` at base). Verified by running the test.
2. **All 12 auto-emitted events recorded at trigger points** — verified by direct inspection of every emit site; multi-path coverage: task.status_changed (8 sites: HTTP, MCP update_task, spawn/completion auto-advance, admin kill, stale-abort, spec-reject, repo-archive), mr.merged/mr.closed (HTTP transition + queue merge + terminal paths), agent.spawned (first spawn, orchestrator tiers, ralph-loop respawn), agent.completed (HTTP + MCP), agent.failed (fail/kill/stale-abort/compose-teardown/spec-reject/repo-archive), merge_queue.processed (merged + every terminal Failed path + operator cancel + circuit-breaker skipped), gate.failed/gate.passed (run_gate), spec.approved, budget.warning (check_spawn_budget, 3 metrics), search.query.
3. **All spec-required properties present** — checked each json! block against the spec table (old_status/new_status/task_id/assigned_to; mr_id/repo_id/gate_count/queue_wait_secs; mr_id/repo_id/reason; agent_id/task_id/compute_target/persona; agent_id/task_id/duration_secs; agent_id/task_id/reason; mr_id/outcome/wait_secs; gate_id/gate_type/mr_id/output_snippet; gate_id/gate_type/mr_id/duration_secs; spec_path/approver_type/approval_mode; workspace_id/metric/threshold_pct; query_length/entity_types/result_count/duration_ms). Orchestrator spawns report `task_id: null` (truthful — orchestrators are not task-bound) rather than a fabricated id.
4. **Query API supports all filter parameters** — event_name (exact + trailing-`*` prefix via LIKE), agent_id, user_id, workspace_id, repo_id, since/until (ISO8601 or unix secs), limit (default 100, cap 10_000), group_by (event_name/agent_id/workspace_id/day, unknown fields rejected). Implemented on SQLite (real Diesel boxed queries), Postgres, and mem adapters with a mem/sqlite semantics-parity test.
5. **Tests pass** — focused probes run here (see Probes); gyre-server suites deferred to the verifier per the review-stage build budget, exact commands recorded in /tmp/stage/review-evidence/summary.md.

## F1 repair verification (this round's independent check)

`record_event` now extracts `AuthenticatedAgent`, ignores body `agent_id`/`user_id`, derives identity from the verified caller (impersonation attributes to the impersonator), and validates `workspace_id`/`repo_id` through `audit.rs::validate_workspace_scope`/`validate_repo_scope` (real lookups, tenant comparison, fail-closed on mismatch or unknown id — the task-102 F2 pattern). Regression tests replicate the original finding's exact repro: forged cross-tenant attribution → 403, unknown id → 404, and `GET /analytics/events?workspace_id=<victim>` returns an empty set; identity binding is asserted on the persisted event via query-back, not just the response.

## Probes (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain analytics` → **3 passed, exit 0**.
- `cargo test -p gyre-adapters analytics` → **13 passed, exit 0** (real SQLite; cross-cutting seed makes every scope predicate non-redundant).
- `cargo check -p gyre-server` → **exit 0**.
- **Mutation probe**: dropped the `workspace_id` predicate from sqlite `query_filtered` → `analytics_query_filtered_all_params` **FAILED** (exit 101); source restored byte-identical. The adapter test kills real regressions.
- Mechanical gates all pass: arch, ABAC route registry, MCP write gates, migration versions + SQL portability, forged-scope-fields, fabricated-scope-defaults, inert-enforcement, mem port contracts, byte-slice truncation, lossy secrets, relative paths, in-memory stores, dead message kinds, forwarded headers. Exemption files: no growth (comment-only edits).
- Task commit attribution: passes against the candidate's committed task file (the working-tree failure is the controller's staged task-file reset, `commits: []`, not a candidate defect — verified by running the check with the committed file content and restoring).

## Test-inflation review

No self-confirming or assertionless tests found: gate tests execute real commands (failing gate runs `false`), budget tests assert both emission-at-threshold and no-emission-below, merge-queue tests drive the production `process_next`, the rate-limit exhaustion test uses a frozen clock (determinism repair), F1 tests query back persisted state, and the containment integration test has positive and negative controls.

## Minor observations (non-blocking, recorded for completeness)

- `search.query`'s event `workspace_id` mirrors the caller-supplied search filter param — pre-existing endpoint semantics (the search handler takes no auth extractor at base); the event truthfully describes the executed search. Search-endpoint scoping itself is a separate concern outside this task.
- `budget.warning` emits on every budget check at/above the 80% threshold rather than only at the crossing instant — a superset of the spec trigger; no missing enforcement.
- gyre-server test execution deferred to the independent verifier (review-stage build budget); required commands recorded in the evidence summary.
