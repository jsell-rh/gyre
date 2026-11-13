# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters (§Purpose context).
Candidate under review: `63632339` (base `349a32a`, 100-commit branch lineage with 21 recorded product commits).
Verdict: **approved** — no blocking findings.

## Probes run (candidate checkout)

- `bash scripts/check-arch.sh` → OK. `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-byte-slice-truncation.sh`, `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-migration-sql-portability.sh`, `check-scope-literal-defaults.sh`, `check-forged-scope-fields.sh`, `check-fail-open-ref-resolution.sh`, `check-conditional-test-guards.sh` → all OK.
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (8m26s).
- `/tmp/stage/bin/cargo test -p gyre-domain analytics` → 3 passed, incl. `analytics_event_matches_spec_schema` (asserts all 9 spec fields + serde round-trip).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib analytics` → 15 passed against real SQLite, incl. `analytics_query_filtered_all_params` (cross-cutting seed: agents span workspaces, workspaces span repos, users span workspaces — every scope predicate individually discriminating, so dropping any predicate flips an assertion) and `analytics_count_grouped_over_full_filtered_set` (grouped totals consistent with `count`; `limit` bounds groups, not the aggregation input).
- Diff inspection of every emit site and every test added in `349a32a..63632339`.

Environmental observations (NOT candidate defects; evidence in `/tmp/stage/review-evidence/probes.txt`):
- `check-assertionless-tests.sh` exits 2 in this sandbox: awk is mawk 1.3.4, the script uses gawk's 3-arg `match($0, /re/, m)`. Script unchanged from base; dies on the first file it scans (`crates/gyre-adapters/src/call_graph.rs`, also unchanged from base).
- `check-task-commit-attribution.sh` fails only because the review harness reset the working-tree `specs/tasks/task-146.md` to `commits: []` (unstaged). At candidate `63632339` the frontmatter records 21 commits and covers all 19 task-labeled product-surface commits the gate flags (verified by script).

## Verified

- **Event Schema.** `AnalyticsEvent` (gyre-domain) carries all 9 spec fields; `with_scope` populates user/session/workspace/repo; SQLite/Postgres rows and migration `2026-10-08-000063_analytics_scope_columns` (unique sequence 000063, portable SQL) add the four scope columns with indexes; mem adapter mirrors. Schema pinned by a serde round-trip unit test.
- **All 12 auto-emitted events at real trigger points with spec-required properties** (each site read, not just grepped):
  - `task.status_changed` — single helper `record_task_status_changed` (props `old_status`/`new_status`/`task_id`/`assigned_to`, scope derived from the persisted Task incl. non-optional `repo_id`) called from the HTTP status endpoint, MCP `update_task`, spawn/completion auto-advance, admin force-kill, stale-abort, spec-reject cancel, repo-archive cancel, and three ralph-loop paths.
  - `mr.merged` — HTTP transition endpoint and merge processor `emit_mr_merged` (props `mr_id`/`repo_id`/`gate_count`/`queue_wait_secs`, wait computed from queue enqueue time).
  - `mr.closed` — HTTP transition, ralph-loop gate-failure close, spec-reject close, repo-archive close (each with a concrete `reason`).
  - `agent.spawned` — worker spawn, orchestrator spawn, ralph-loop re-spawn; `compute_target` reports the actual launch label (not hardcoded), `persona` reports the bound persona slug.
  - `agent.completed` / `agent.failed` — HTTP complete, MCP `gyre_agent_complete`; fail/kill covered across the fail/stop endpoints, agent-status endpoint, admin force-kill, stale-abort, spec-reject, repo-archive, compose teardown, ralph-loop ceilings, worker failed-launch teardown, orchestrator death monitor. Unresolvable MR scope on gate events skips emission rather than fabricating `"default"` (exemption count lowered accordingly).
  - `merge_queue.processed` — merged group path, single-entry path, and every terminal Failed path through `emit_queue_processed_terminal` (outcomes merged/failed/skipped per spec).
  - `gate.failed`/`gate.passed` — gate executor after real gate execution, with `output_snippet` (UTF-8-safe `truncate_bytes`) and `duration_secs`.
  - `spec.approved` — approve handler, props `spec_path`/`approver_type`/`approval_mode` from the ledger entry.
  - `budget.warning` — budget check + usage-recording paths at the 80% threshold, props `workspace_id`/`metric`/`threshold_pct`.
  - `search.query` — HTTP search handler and MCP `gyre_search`, both through `record_search_query` with caller-bound identity and pre-validated workspace scope (cross-tenant/unknown scope is rejected before the search runs, and the validation survives facet-override of the filter).
- **Query API / Query Parameters.** `GET /api/v1/analytics/events` supports `event_name` (exact + trailing-`*` prefix via LIKE on SQLite and Postgres), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`/`until` (ISO8601 or unix secs; date-only upper bound extends to end-of-day), `limit` (default 100, capped 10 000), and `group_by` (`event_name`/`agent_id`/`workspace_id`/`day`, unsupported values rejected) aggregated server-side over the full filtered set with `limit` bounding groups. `/count` and `/daily` (incl. spec form `?event_name=&days=`) unchanged in contract. All three adapters (SQLite, Postgres, mem) implement `query_filtered`/`count_grouped` with mirrored WHERE semantics; the mem adapter is parity-tested against the SQLite filter semantics.
- **Anti-forgery.** `POST /analytics/events` ignores body `agent_id`/`user_id` (identity derived from the verified caller, impersonation attributed to the impersonator) and validates `workspace_id`/`repo_id` against the caller's tenant — the same rule as audit events. No new endpoints were created, per the task instructions.
- **Tests are real, not inflated.** Per-event tests drive the production path (router `oneshot` requests or the real spawn/merge/gate cores) and then query `state.analytics` asserting event count, name, and each spec-required property (e.g. admin kill asserts `reason == "force-killed by admin"`; compose teardown asserts one event per killed agent; grouped-count tests assert full-set totals that a limit-truncated implementation would get wrong). The sqlite scope-filter test's cross-cutting seed makes every predicate individually load-bearing.
- **Coverage matrix updates are honest** — sections 1–5 marked `implemented` with landed evidence; sections 6–12 remain `task-assigned` for tasks 147–149; `spec-links.md` 92%→100% is a side effect of the shared `IndexingAnalyticsRepository` delegating the new port methods (no hollow marking).
- **Scope discipline.** Script/exemption-file edits only lower counts and reword comments (no new exemptions); rust_extractor/recovery/middleware changes are clippy determinism/comment/logging only; no frontend source changes.

## Notes for the verifier (not runnable in the review sandbox — 4 GiB budget)

Full `cargo test --all` plus the focused gyre-server suites listed in `/tmp/stage/review-evidence/probes.txt` (analytics, budget, specs approve/reject, merge_requests, merge_processor, gate_executor, spawn, orchestrator, ralph_loop, stale_agents, compose, admin, search, mcp, api_integration) must run in the independent verifier. Prior verification run `c1ef1158` already exercised `cargo test --all` on this lineage and its two failures were repaired in `f1775677` (repo scope on `task.status_changed`, grouped-filter test literal); `cargo check -p gyre-server` passes on the exact candidate.

Minor observations (non-blocking): auto-emitted events leave `user_id`/`session_id` NULL on background paths where no human caller exists (spec marks them "if applicable"; caller-attributing paths do populate them); `spec.approved` fires per approval action (including partial approvals in `human_and_agent` mode), which matches the per-approver `approver_type` property semantics.
