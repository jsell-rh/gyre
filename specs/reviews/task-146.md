# Review — task-146 (Complete analytics event schema and auto-emitted events coverage)

Spec: `specs/system/analytics.md` §Event Schema, §Auto-Emitted Events, §Query API, §Query Parameters.
Candidate under review: `0316693` (base `d9c419a`, the task-191 budget-enforcement base; this round = rebase resolution `c65edaca`/`1259205d` + repair `f1775677`).
Verdict: **needs-revision** — two material findings, both missing `agent.failed` trigger points. The rest of the analytics surface is verified clean and strongly tested.

## Probes run (checkout at candidate `0316693`)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --quiet` → **exit 0** (`/tmp/stage/review-evidence/cargo-check-gyre-server.log`).
- `cargo test -p gyre-domain --quiet` → **508 + 8 passed, 0 failed** (includes `analytics_event_matches_spec_schema`).
- `cargo test -p gyre-adapters --quiet analytics` → **15 passed, 0 failed** (SQLite `query_filtered` all-params, `count_grouped` over full filtered set, by-day grouping).
- `bash scripts/check-migration-versions.sh` → OK (no duplicate Diesel versions; the renumbered `2026-10-08-000063_analytics_scope_columns` is unique).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-fabricated-scope-defaults.sh` → OK.
- Diff inspection of `f1775677` (repo scope on `task.status_changed`, discriminating grouped-filter test) and of all 12 event emission sites against `analytics.md`.
- gyre-server test compilation deferred to the independent verifier (4 GiB budget) — see "Verifier must run" below.

## Verified working (no findings)

- **Event schema**: `AnalyticsEvent` (`gyre-domain/src/analytics.rs`) carries all 9 spec fields (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`, `repo_id`, `properties`, `timestamp`); `with_scope()` populates the scope ids; serde round-trip tested. `Option<String>` for the id fields is serde-equivalent to the spec's `Option<Id>` (Id is a string newtype) — acceptable.
- **Emissions verified at real trigger points with spec-required properties**: `task.status_changed` (shared helper `record_task_status_changed`, HTTP + MCP + spawn/complete auto-advance + admin kill + stale abort + spec rejection + repo archive, now carrying repo scope from the persisted entity), `mr.merged` (HTTP transition endpoint AND merge-queue path via `emit_mr_merged`, with real `gate_count` from `gate_results.list_by_mr_id` and `queue_wait_secs` from enqueue time), `mr.closed` (HTTP close path), `agent.spawned` (with resolved `compute_target` label and bound-persona label, including ralph-loop re-spawns and orchestrator spawns), `agent.completed` (HTTP complete + MCP `gyre_agent_complete`), `agent.failed` (admin kill, fail/stop endpoints, compose teardown, spec-rejection stop, repo-archive stop, orchestrator process-death monitor, ralph-loop iteration ceiling, terminal re-spawn failure, heartbeat-timeout Abort), `merge_queue.processed` (merged/failed/skipped outcomes, group + single paths), `gate.failed`/`gate.passed` (gate executor loop, real `gate_id`/`gate_type`/`mr_id`, `output_snippet` byte-truncated, scope resolved from the MR), `spec.approved` (approval handler, approver identity split human/agent), `budget.warning` (80% threshold from the budget check/usage paths), `search.query` (HTTP search + MCP `gyre_search`, caller-bound identity, validated workspace scope).
- **Query API**: all spec parameters (`event_name` exact/prefix-`*`, `agent_id`, `workspace_id`, `repo_id`, `since`/`until` ISO8601 + unix seconds, `limit` default 100/max 10000, `group_by` ∈ {event_name, agent_id, workspace_id, day}) implemented in the HTTP layer and in **all three adapters** (mem, SQLite, Postgres) with real semantics; grouped counts aggregate the full filtered set server-side with `limit` bounding output groups only; unknown `group_by` → 400. Migration `000063_analytics_scope_columns` is portable (plain `ALTER TABLE`/`CREATE INDEX IF NOT EXISTS`).
- **Anti-forgery**: `POST /analytics/events` ignores request-body `agent_id`/`user_id` (binds to verified caller) and validates `workspace_id`/`repo_id` against the caller's tenant — forged-scope rejection tested with victim-scope-stays-empty negatives. `search.query` follows the same rule.
- **Tests are real**: every emission test queries `state.analytics` populated by the production handler on the same state, so deleting an emission fails the test; property assertions mostly check values from the trigger (`gate_count == 2` against seeded gate results, persona slug pinned, `old_status`/`new_status`/`assigned_to` by value, grouped totals cross-checked against `/count`). Adapter tests use a deliberately cross-cutting seed (dropping any one predicate changes the result).

## Findings

- [-] **F1 (major): `agent.failed` is not recorded on the budget-enforcement kill path.** `budget_stop_agent` (`crates/gyre-server/src/api/budget.rs:1416`, triggered at `:667` lifetime-exceeded and `:1403` beyond-limit/grace-expiry via `stop_workspace_agents`) kills the agent's process via `ComputeTarget::kill_process`, transitions it to `Stopped`, revokes its token, and emits the `AgentKilled` audit event with actor `"budget-enforcement"` — but never records the spec-required `agent.failed` analytics event (analytics.md §Auto-Emitted Events: trigger "**Agent fails or is killed**"). Every other kill/stop path emits it, including paths with the identical `Stopped` terminal status and an explicit "is a kill path" comment (`stop_agent` at `spawn.rs:2733` with reason `"killed"`, repo-archive stop at `repos.rs:430`). The task's own "Shipped" claim ("all 12 auto-emitted events … at their real trigger points") is therefore false for this trigger, and the agent-consumable decision loops (`agent.performance` failure rate, `budget.headroom`) silently undercount budget-killed agents. This round rebased onto the budget-enforcement base and touched this exact file (`emit_budget_warning` lives in the same enforcement path), so the gap is squarely in the task's contract. Evidence: `/tmp/stage/review-evidence/f1-budget-kill-no-agent-failed.txt`. **Repair:** call `record_agent_failed` (or emit inline with `task_id`, `reason` = the budget stop reason) inside `budget_stop_agent` after the transition persists, and add a test that drives `check_spawn_budget`/grace-expiry into a kill and asserts the `agent.failed` event with its reason.
- [-] **F2 (major): `agent.failed` is not recorded on the heartbeat-timeout Pause path.** `stale_agents.rs:208` (`DisconnectedBehavior::Pause`) transitions the agent to `Stopped` — the code itself documents it as "a terminal stop — the agent's run ends here" — and revokes its task secrets, but records no `agent.failed`, while the sibling Abort arm (`:68`) emits with reason `"heartbeat timeout (abort)"`. Same underlying failure (heartbeat loss), same terminal run-end, inconsistent tracking. Evidence: `/tmp/stage/review-evidence/f2-pause-arm-no-agent-failed.txt`. **Repair:** emit `agent.failed` (reason e.g. `"heartbeat timeout (pause)"`) in the Pause arm and cover it in the existing stale-agent test module.

Minor (non-blocking, recorded for completeness):

- `compare_returns_change_pct` (`api/analytics.rs` test mod) is self-confirming: it seeds nothing and asserts `change_pct.is_null() || is_number()` — a tautology. The `/compare` endpoint is outside this task's five coverage sections (not in analytics.md §Query API), so it does not affect this verdict, but it should not be cited as coverage.
- A few emission tests assert presence-only on secondary properties (`duration_secs`, `output_snippet`, `reason`, `compute_target`); core ids/values are pinned. Low risk.
- Cross-adapter divergences, neither a spec-filter gap: SQLite `LIKE` prefix match is ASCII-case-insensitive (mem/PG are case-sensitive); Postgres `group_by=day`/`aggregate_by_day` render in the server/session timezone rather than forced UTC (SQLite/mem are strictly UTC).
- `merge_queue.processed` on the HTTP-merged-without-queue path reports `queue_wait_secs: null` (MR never queued) — honest and acceptable.

## Verifier must run (deferred from this sandbox, 4 GiB budget)

- `cargo test -p gyre-server --lib api::analytics`
- `cargo test -p gyre-server --lib api::search`
- `cargo test -p gyre-server --lib api::specs`
- `cargo test -p gyre-server --lib gate_executor`
- `cargo test -p gyre-server --lib api::budget`
- `cargo test -p gyre-server --lib api::spawn`
- `cargo test -p gyre-server --lib api::merge_requests`
- `cargo test -p gyre-server --lib api::merge_queue`
- `cargo test -p gyre-server --lib stale_agents`
- `cargo test -p gyre-server --lib merge_processor`
- `cargo test -p gyre-server --lib mem` (analytics query/group_by parity tests)
- `cargo test -p gyre-server --test api_integration analytics`

Generated-output note: `web/dist/` was not evaluated (owned by the deterministic verifier); no frontend source changes are part of this task's surface.
