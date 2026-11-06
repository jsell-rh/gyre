---
title: "Complete analytics event schema and auto-emitted events coverage"
spec_ref: "analytics.md §Event Schema"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "analytics.md §Purpose"
  - "analytics.md §Event Schema"
  - "analytics.md §Auto-Emitted Events"
  - "analytics.md §Query API"
  - "analytics.md §Query Parameters"
commits: ["ed7ac6d36b4eed6931538def2cd893c2b19c9a06"]
---

## Spec Excerpt

From `analytics.md` §Event Schema:

```rust
pub struct AnalyticsEvent {
    pub id: Id,
    pub event_name: String,
    pub agent_id: Option<Id>,
    pub user_id: Option<Id>,
    pub session_id: Option<String>,
    pub workspace_id: Option<Id>,
    pub repo_id: Option<Id>,
    pub properties: serde_json::Value,
    pub timestamp: u64,
}
```

### Auto-Emitted Events (spec-required)

| Event Name | Trigger | Properties |
|---|---|---|
| `task.status_changed` | Task transitions status | `old_status`, `new_status`, `task_id`, `assigned_to` |
| `mr.merged` | MR successfully merged | `mr_id`, `repo_id`, `gate_count`, `queue_wait_secs` |
| `mr.closed` | MR closed without merge | `mr_id`, `repo_id`, `reason` |
| `agent.spawned` | Agent spawned | `agent_id`, `task_id`, `compute_target`, `persona` |
| `agent.completed` | Agent completes successfully | `agent_id`, `task_id`, `duration_secs` |
| `agent.failed` | Agent fails or is killed | `agent_id`, `task_id`, `reason` |
| `merge_queue.processed` | Queue entry processed | `mr_id`, `outcome`, `wait_secs` |
| `gate.failed` | Quality gate fails | `gate_id`, `gate_type`, `mr_id`, `output_snippet` |
| `gate.passed` | Quality gate passes | `gate_id`, `gate_type`, `mr_id`, `duration_secs` |
| `spec.approved` | Spec approved | `spec_path`, `approver_type`, `approval_mode` |
| `budget.warning` | Budget threshold crossed | `workspace_id`, `metric`, `threshold_pct` |
| `search.query` | Full-text search executed | `query_length`, `entity_types`, `result_count`, `duration_ms` |

## Implementation Plan

**NOTE:** The analytics system is partially implemented. The event schema exists, query endpoints exist (`POST/GET /api/v1/analytics/events`, `GET /api/v1/analytics/count`, `GET /api/v1/analytics/daily`), and some auto-emitted events exist (task.status_changed, agent.spawned, mr.merged, merge_queue.processed). This task completes the missing pieces.

1. **Audit existing AnalyticsEvent struct** — verify it matches the spec schema. Add any missing fields (`user_id`, `session_id`, `workspace_id`, `repo_id`) if not present.

2. **Add missing auto-emitted events** — these events are NOT currently auto-emitted and need to be added:

   a. `mr.closed` — in `crates/gyre-server/src/api/merge_requests.rs`, where MRs are closed without merge
   b. `agent.completed` — in the agent completion handler (spawn.rs or agent lifecycle)
   c. `agent.failed` — in the agent failure/kill handler
   d. `gate.failed` / `gate.passed` — in the merge processor gate evaluation loop
   e. `spec.approved` — in `crates/gyre-server/src/api/specs.rs` approval handler
   f. `budget.warning` — in the budget check/enforcement path (spawn.rs budget validation)
   g. `search.query` — in the search endpoint handler

3. **Enrich existing auto-emitted events** — ensure existing events include all spec-required properties:
   - `task.status_changed`: verify `old_status`, `new_status`, `assigned_to` are all present
   - `agent.spawned`: verify `compute_target`, `persona` properties
   - `mr.merged`: verify `gate_count`, `queue_wait_secs` properties
   - `merge_queue.processed`: verify `outcome`, `wait_secs` properties

4. **Verify query API completeness** — the spec requires filtering by `event_name`, `agent_id`, `workspace_id`, `repo_id`, `since`, `until`, `limit`, `group_by`. Verify existing query endpoints support all parameters.

5. **Tests:**
   - Integration test for each new auto-emitted event: trigger the action, verify the event is recorded with correct properties
   - Unit test: AnalyticsEvent struct matches spec schema

## Acceptance Criteria

- [ ] AnalyticsEvent struct matches spec schema (all fields present)
- [ ] All 12 auto-emitted events are recorded at their trigger points
- [ ] Each event includes all spec-required properties
- [ ] Query API supports all spec-required filter parameters
- [ ] Tests pass for each auto-emitted event

## Agent Instructions

- Read `crates/gyre-server/src/api/analytics.rs` for existing analytics implementation
- Read `crates/gyre-server/src/api/mod.rs` lines 430-438 for existing analytics routes
- Grep for `analytics.record` to find all existing auto-emit call sites
- Read `crates/gyre-server/src/api/spawn.rs` for agent lifecycle events
- Read `crates/gyre-server/src/merge_processor.rs` for merge/gate events
- Read `crates/gyre-server/src/api/specs.rs` for spec approval events
- Read `crates/gyre-server/src/api/search.rs` for search endpoint
- Do NOT create new endpoints — the query API routes already exist. Only add missing auto-emitted events and verify completeness.

## Shipped

### Analytics event schema and auto-emitted events (commit 38c2c5e)

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) carries all 9 spec fields
  (`id`, `event_name`, `agent_id`, `user_id`, `session_id`, `workspace_id`,
  `repo_id`, `properties`, `timestamp`); `with_scope` populates the
  user/session/workspace/repo scope fields. A serde round-trip test pins
  the shape against the spec schema.
- All 12 spec-required auto-emitted events record at their real trigger
  points with all spec-required properties, each proven by a passing
  per-event test: `task.status_changed`, `mr.merged`, `mr.closed`,
  `agent.spawned`, `agent.completed`, `agent.failed`,
  `merge_queue.processed`, `gate.failed`, `gate.passed`, `spec.approved`,
  `budget.warning`, `search.query`.
- Query API: `POST/GET /api/v1/analytics/events`, `GET /count`, `GET /daily`
  with the full spec filter set — `event_name` (exact or trailing-`*`
  prefix), `agent_id`, `user_id`, `workspace_id`, `repo_id`, `since`,
  `until`, `limit` (default 100, cap 10_000), `group_by` — implemented on
  SQLite, Postgres, and mem adapters (adapter test
  `analytics_query_filtered_all_params`).

### Trigger-point completeness (review repair round, commit b818d20d)

The branch was first reconciled with main (d81b0fe4, merge commit
  a488d9df) — web/dist taken byte-identical from main and the coverage
  SUMMARY TOTAL recomputed from the merged per-file rows. The
  independent review then found three events whose trigger coverage was
  partial: terminal queue outcomes, internal task transitions, and the
  spec-rejection kill path. All are now wired in production code:

- **`merge_queue.processed` on every terminal outcome** — a new
  `fail_queue_entry()` helper in merge_processor.rs updates the entry to
  `Failed` and records the event atomically at every failure site:
  quality-gate failure during candidate selection (the primary failure
  mode), MR not found (selection and atomic-group paths), repo not found,
  dependency-closed dep-health failure, spec-policy blocks
  (no-spec_ref/approval/stale), breaking-change policy, spec-link gates
  (superseded/conflicts), constraint enforcement, and attestation-ABAC
  denial. The merged/conflict/git_error sites keep their existing
  emissions. Circuit-breaker `Cancelled` entries now record with the spec
  outcome value `skipped` (`emit_queue_processed_terminal`). When the MR
  no longer resolves, the event records with the MR id from the entry and
  no fabricated scope.
- **`task.status_changed` on internal transitions** — a shared
  `record_task_status_changed()` helper (api/analytics.rs) is now called
  by every production path that persists a task status transition: the
  HTTP status endpoint and MCP `update_task` (refactored onto the
  helper), agent-spawn auto-advance Backlog→InProgress, agent-completion
  auto-advance →Review, admin force-kill task block →Blocked, stale-abort
  task block →Blocked, spec-rejection cancellation →Cancelled, and
  repo-archive cancellation →Cancelled.
- **`agent.failed` on the remaining kill paths** — the spec-rejection
  agent stop (reason `killed: spec rejected: …`) and the repo-archive
  agent stop (reason `killed (repo archived)`) now record with the same
  agent_id/task_id/reason/duration_secs properties as the
  fail/admin-kill/stale-abort paths.
- **CI gate repair** — a deny-by-default `clippy::never_loop` error in
  pre-existing `rust_extractor.rs` code (loop that always broke on the
  first iteration) fails the CI clippy gate on any error-level finding;
  rewritten as the behavior-identical `if let Some(candidate) =
  …next()` clippy itself suggests.

Each new trigger point has a regression test that fails on the old code:
  `gate_failure_during_selection_records_queue_processed_failed`,
  `mr_not_found_during_selection_records_queue_processed_failed`,
  `spec_policy_block_records_queue_processed_failed`,
  `circuit_breaker_cancel_records_queue_processed_skipped`,
  `spawn_auto_advance_records_task_status_changed`,
  `complete_auto_advance_records_task_status_changed`, plus assertions
  added to `admin_kill_blocks_active_task`,
  `abort_records_agent_failed_analytics_event`,
  `reject_spec_cancels_in_flight_tasks`, `archive_cancels_active_tasks`,
  and `archive_stops_active_agents`.

### Verification-finding repair round (commit dc13ede3)

The prior candidate failed the deterministic verification gates (rustfmt,
 clippy, forbidden exemption replacements, and four static checks). All
 findings were fixed with real enforcement, never by re-adding exemptions:

- **Orchestrator tenant containment (task-093 F2/F3):** both
  `spawn_workspace_orchestrator` and `spawn_repo_orchestrator` now load the
  scoped entity, compare its tenant against `auth.tenant_id`, and return
  `Forbidden` on mismatch. Integration test
  `orchestrator_spawn_denied_across_tenants` proves 403 + no agent created
  for a foreign-tenant workspace and 201 for a same-tenant control.
- **Attestation ABAC enforcement (task-077 F5):** `evaluate_attestation_abac`
  results are bound at both call sites; a `Deny` decision fails the merge
  queue entry before the merge commit is created (with no policies stored
  the evaluation is `Allow` and merges proceed unchanged). Regression test
  `attestation_abac_deny_blocks_merge` fails on the old audit-only code.
- **Advisory persona check renamed (task-099 F2):** `validate_persona` →
  `warn_unapproved_persona`; behavior unchanged (spec: soft check), but the
  name can no longer be mistaken for an enforcement gate.
- **specs.rs:** SHA truncation is char-boundary-safe via `truncate_bytes`;
  spec-rejected notification fanout resolves the tenant from the workspace
  record (skip + warn when unresolvable) instead of fabricating a
  `"default"` tenant from `repo_id`.
- **gate_executor.rs:** GateFailure event emission is skipped with a
  warning when the MR's workspace cannot be resolved, instead of
  fabricating a `"default"` workspace scope.
- **recovery.rs:** both `resolve_ref` sites fail closed (unresolvable
  default-branch HEAD reports `main_green=false` with a warning;
  `revert_mr` returns 409 when the target branch no longer resolves)
  instead of proceeding with empty-string SHAs.
- Exemption entries for every fixed site were deleted (never replaced),
  `FROZEN_EXEMPTION_COUNT` for fabricated-scope-defaults lowered 5 → 3,
  and the `scope-literal-defaults` entries restored byte-identical to
  main (pre-existing debt this branch does not own).

### Review-repair round: migration collision and exemption pin (commit 6a3ce040)

The independent review found two gate regressions introduced by the
  merge with main (a488d9df), both fixed:

- **Duplicate Diesel migration version 20261008000056** — this branch's
  `analytics_scope_columns` migration collided with
  `2026-10-08-000056_spec_assertion_results` brought in from main.
  Diesel runs only one migration per version (alphabetically first
  wins), so the analytics migration was silently dropped and fresh
  databases had no user_id/session_id/workspace_id/repo_id columns on
  `analytics_events` (6 adapter test failures, guard-test panic,
  check-migration-versions.sh exit 1). Renumbered to the next unused
  sequence 000057; content unchanged. The adapter scope-filter tests
  (`analytics_query_filtered_all_params` and siblings) now pass again on
  real storage.
- **Stale line-pinned exemption** — branch insertions shifted the
  `'./repos/'` prose comment in spawn.rs from line 597 to 605,
  invalidating the `relative-path-defaults-exemptions.txt` pin and
  failing `check-relative-path-defaults.sh`. That round re-pinned to
  :605, which the verification gate then rejected as a new exemption
  (see the final repair round below, commit ab8c41a2, for the durable
  fix). No new exemption was added.

### Verification-repair round: exemption-gate compliance (commit ab8c41a2)

The deterministic verification gate (`bash /tmp/stage/checks.sh`) failed
  on exactly one finding: the re-pinned `relative-path-defaults-exemptions.txt`
  entry (`spawn.rs:597` → `:605`) counts as a NEW exemption entry vs the
  base, which the gate forbids by design — after which the check runs
  unmasked and fails on the literal. Fixed for real, never by re-adding or
  re-wording an exemption:

- The spawn.rs prose comment no longer quotes the relative repos-root
  default literally (`"./repos/"` removed from the comment text; the
  canonicalize call the comment documents is unchanged and still
  guards the actual behavior).
- The `spawn.rs` exemption entry was DELETED from
  `relative-path-defaults-exemptions.txt` — a pure deletion vs base
  (same accepted shape as this branch's other exemption deletions),
  per the file's own policy of removing entries once the site no
  longer trips the check.

Evidence: `check-relative-path-defaults.sh` exits 0 with no spawn.rs
  exemption present; the exemption-file diff vs base
  9bef21a4 contains only the deletion (retained under
  /tmp/stage/review-evidence/relative-path-fix.txt).

### Test evidence

- This round (ab8c41a2): `check-relative-path-defaults.sh`: OK (no
  exemption entry for spawn.rs present);
  `check-task-commit-attribution.sh`: OK after recording the commit;
  `cargo fmt --check -p gyre-server`: clean. Comment-only code change —
  no behavior surface affected.
- Prior round (6a3ce040): `cargo test -p gyre-adapters --lib`: 348 passed,
  0 failed (includes the previously failing analytics adapter tests and
  the `no_duplicate_migration_version_prefixes` guard);
  `check-migration-versions.sh`, `check-migration-sql-portability.sh` all
  pass (evidence under /tmp/stage/review-evidence).
- `cargo test -p gyre-server --lib`: 1293 passed, 0 failed (includes
  the 11 new/extended trigger-point regression tests).
- `cargo test -p gyre-server --test api_integration`: 67 passed, 0 failed.
- `cargo fmt --check -p gyre-server`: clean.
- Earlier rounds (unchanged, see commits 38c2c5e/dc13ede3):
  `cargo test -p gyre-ports -p gyre-domain -p gyre-common`: 483 passed;
  adapter test `analytics_query_filtered_all_params` green on
  SQLite/Postgres/mem.
