# Review — task-191 (Implement budget enforcement actions: warn/graceful-stop with grace, kill, and reassignment)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Enforcement Behavior (BudgetEnforcementAction enum; 80% Warn with BudgetWarning message/notification; 100% GracefulStop with BudgetExhausted, 60s grace, kill, work preserved, reassignment task).

Commits under review: candidate `da6f23bb5` on base `e72777d4c` — product commit `b3dea2900` (recorded in `commits:` frontmatter) plus docs/process commits `9f2447e4`/`188aaea9`/`da6f23bb5` that touch only `specs/tasks/task-191.md`. Cumulative diff: +1170/−45 across `gyre-domain/src/budget.rs`, migration `000058`, SQLite/PG `budget.rs` adapters, `gyre-server/src/api/{budget,admin}.rs`, `mcp.rs` (signature widening only), `docs/api-reference.md`, coverage row 28.

Verdict: **complete** (approved). Full probe log: `/tmp/stage/review-evidence/task-191-review-evidence.md`.

## Evidence

- **Focused suite (unmodified candidate):** `cargo test -p gyre-server --lib api::budget` — 17 passed, 0 failed, including all four acceptance-criteria scenarios (85% warn with message + spawner notification and no kill; below-threshold silence; ≥100% graceful stop with `Dead`, counter 0, worktree record intact, reassignment task titled per spec; HardKill inline / QueuePause kv-pause / cost-limit / serde / API-persistence).
- **Mutation probes (each restored byte-identical, `sha256sum -c` + clean `git status` verified):**
  - Removing the `check_and_enforce_budget` call from `record_llm_call_usage` fails the warn-funnel test (0 messages emitted) — the production funnel genuinely drives enforcement, not just the direct-call tests.
  - Removing both per-period de-dup guards fails both no-re-fire assertions — de-duplication is real, not decorative.
  - Forcing worktree cleanup in the shared `kill_agent` fails the worktree-preservation assertion — preservation is enforced by `preserve_worktree`, not by absence of cleanup code.
- **Kill-helper cutover:** `api::admin` 33 passed, `api::spawn` 36 passed after `admin_kill_agent` was migrated to the extracted `kill_agent` (clean cutover, no forked kill sequence; the helper additionally releases the budget slot the old admin path leaked — an improvement, guarded to `Active`-status agents so no double release).
- **Persistence:** `gyre-adapters` `enforcement_action_round_trips_through_sqlite` passes (set → get → overwrite-to-None upsert clear); PG row mapping is byte-parallel to SQLite; migration `000058` is portable plain `ALTER TABLE` (`check-migration-sql-portability.sh` and `check-migration-versions.sh` exit 0).
- **Mechanical gates:** arch, dead-message-kinds (BudgetWarning/BudgetExhausted now have emitters), inert-enforcement, unwired-port-methods, mem-port-contracts, task-commit-attribution, ABAC registry/exempt-handlers, api-auth, scope-literal/fabricated-scope defaults, in-memory-state-stores (the de-dup marker is `kv_store` port-backed), event-payload, message-delivery — all exit 0. `silent-result-discard`/`conditional-test-guards` report only pre-existing findings, none on lines added by this candidate. Three notification/event scripts abort on mawk syntax errors in this sandbox — infrastructure, not code.

## Verified against the task contract

- Enum in `gyre-domain` with all four variants + serde snake_case round-trip; `enforcement_action` optional on `BudgetConfig` (None = spec default) threaded through migration, both adapters, and the budget API DTOs.
- Single central helper; grep confirms `record_llm_call_usage` is its only production caller and no call site re-implements thresholds.
- Utilization = max over token/cost limits with None and zero guards; ≥80%/<100% Warn (message per active agent + priority-7 `NotificationType::BudgetWarning` to unique spawners, no kill); ≥100% default GracefulStop (BudgetExhausted message → 60s injectable grace → re-listed still-Active agents killed via shared helper, counters decremented, worktree preserved, `"Agent hit budget limit on {task}, needs reassignment or budget increase"` task created with `parent_task_id` link).
- Overrides honored: HardKill inline (no timer), QueuePause via the existing `merge_processor::pause_merge_queue` mechanism (kv + `MergeQueuePaused` event, no kill), Warn at 100% warns only.
- De-dup via persisted kv marker keyed to `period_start` — thresholds can re-fire after the midnight reset, never spam within a period (mutation-verified).
- Emitted payloads conform to `MessageKind::payload_schema` for both kinds; messages persist through the Event tier (not telemetry), notifications through the port.

## Non-blocking notes (recorded, not findings)

- The ≥100% tests invoke `check_and_enforce_budget_with_grace` directly rather than through `record_llm_call_usage`; the funnel wiring itself is proven by the warn-tier test failing under Mutation A, and both tiers share the single funnel call, so this is a test-shape choice, not a coverage hole.
- The grace timer is in-process (`tokio::spawn` + `sleep`), exactly as the task plan specifies; a server restart during the 60s grace orphans that period's kill (the persisted `exhausted` marker prevents re-fire). Durable grace scheduling would be a follow-up hardening, not a contract requirement here.
- Enforcement is workspace-keyed, per the task plan; tenant/repo-level configurability remains unthreaded (repo-level `budget` field absent) — honestly declared in coverage row 28 and not part of this task's acceptance criteria.
- A budget kill leaves the killed agent's original task `InProgress` (admin kill blocks it); blocking would conflict with the reassignment intent, and the spec is silent — conscious choice, reassignment task carries the linkage.
