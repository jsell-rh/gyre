# Review: TASK-146 — Complete Analytics Event Schema and Auto-Emitted Events Coverage

**Reviewer:** Independent review (resumed round; prior interrupted-review probe evidence revalidated first-hand)
**Date:** 2026-11-05
**Comparison base:** d81b0fe4ab93abff06089432e4a8fca5be6e5d31
**Candidate (HEAD):** 306fbf0a2d35ee813dbc70f9c62cab12df68083e
**Verdict:** changes_requested

---

## Findings

### F1 (blocker, production defect): duplicate migration version silently drops the analytics scope columns

`crates/gyre-adapters/migrations/2026-10-08-000056_analytics_scope_columns` (added by this
branch in commit 38c2c5e) collides on Diesel version `20261008000056` with
`2026-10-08-000056_spec_assertion_results`, which main (d81b0fe4, task-180) already owned
before the reconciliation merge a488d9df. Diesel runs only one migration per version and
silently drops the other; empirically the analytics migration is the dropped one, so
`analytics_events` never gains the `user_id`/`session_id`/`workspace_id`/`repo_id` columns
on fresh databases.

Evidence (all commands run against the exact candidate 306fbf0a; base outputs captured from
a clean worktree of d81b0fe4):

- `cargo test -p gyre-adapters --lib analytics` → exit 101, **6 of 13 tests fail**:
  `no such column: analytics_events.user_id`,
  `table analytics_events has no column named workspace_id`
  (incl. `analytics_query_filtered_all_params`, which the shipped notes claim is green).
- `cargo test -p gyre-adapters --lib no_duplicate_migration_version_prefixes` → exit 101,
  panics listing both directories under version 20261008000056.
- `bash scripts/check-migration-versions.sh` → exit 1 (exit 0 at base).
- Saved under `/tmp/stage/review-evidence/`: `analytics-adapter-tests-candidate.out`,
  `guard-test-candidate.out`, `check-migration-versions-candidate-rerun.out`,
  `check-migration-versions-base.out`.

Impact: the task's central deliverables are broken at the storage layer on real SQLite
deployments — the spec schema's scope fields cannot persist and the Query API's
`user_id`/`workspace_id`/`repo_id` filters fail with SQL errors. The shipped test evidence
(`gyre-server --lib` 1293 passed, `gyre-ports/domain/common` 483 passed) omitted
`gyre-adapters`, the one crate whose tests catch this; the "adapter test green (earlier
rounds, unchanged)" claim is stale — the merge with main changed the migration set after
that round. Acceptance criteria "Query API supports all spec-required filter parameters"
and "Tests pass for each auto-emitted event" are not met on real storage (gyre-server
tests use the mem adapter, which has no schema and masks the defect).

Fix: renumber the analytics migration to the next unused sequence (`000057`), both up and
down SQL, and re-run the adapter suite.

### F2 (CI gate regression): line-pinned relative-path exemption invalidated by line shift

This branch's insertions in `spawn.rs` moved the pre-existing `"./repos/"` prose comment
from line 597 (where the unchanged `scripts/relative-path-defaults-exemptions.txt` pins
it) to line 605. `bash scripts/check-relative-path-defaults.sh` now exits 1 flagging
`spawn.rs:605`; it exits 0 at base (evidence: `check-relative-path-candidate-rerun.out`,
`check-relative-path-base.out`). The candidate fails a pre-commit/CI gate that main passes.

Fix: update the exemption entry to the new line (or add the `// path:ok` inline marker).

---

## What was verified as sound (spot-checks, not waivers)

- `AnalyticsEvent` (gyre-domain/src/analytics.rs) matches the spec schema: all 9 fields,
  `with_scope` populates user/session/workspace/repo scope.
- All 12 spec-required events have real production emission sites with the spec-required
  properties: `task.status_changed` (shared `record_task_status_changed` helper called
  from the HTTP status endpoint, MCP `update_task`, spawn auto-advance, completion
  auto-advance, admin force-kill, stale-abort, spec rejection, repo archival),
  `mr.merged`/`mr.closed` (merge_requests.rs status endpoint, both with full property
  sets), `agent.spawned` (worker + orchestrator paths), `agent.completed`/`agent.failed`
  (completion/fail/stop/admin-kill/stale-abort/spec-rejection/archive paths),
  `merge_queue.processed` (merged/conflict/git_error plus a `fail_queue_entry` helper at
  15 failure sites, circuit-breaker `skipped` outcome), `gate.failed`/`gate.passed`
  (gate_executor), `spec.approved` (specs.rs approval handler),
  `budget.warning` (budget threshold helper), `search.query` (search endpoint).
- Query API params all present (`event_name`, `agent_id`, `user_id`, `workspace_id`,
  `repo_id`, `since`, `until`, `limit`, `group_by`) — but see F1: on SQLite they cannot
  work because the backing columns are missing.
- Other exemption-touching static checks pass on the candidate: fabricated-scope-defaults,
  inert-enforcement, byte-slice-truncation, abac-exempt-handlers, abac-route-registry,
  scope-literal-defaults, fail-open-ref-resolution, arch, migration-sql-portability,
  mcp-write-tools, lossy-secret-conversion, dead-message-kinds.

The trigger-point wiring itself is real and complete; the rejection is entirely on the
storage-layer collision (F1) and the CI gate regression (F2). Both were verified passing
at the assigned base.

## Verifier note

After F1's renumber fix, re-run: `cargo test -p gyre-adapters --lib` (full crate — the
guard test and all analytics tests), `bash scripts/check-migration-versions.sh`, and
`bash scripts/check-relative-path-defaults.sh`; plus the gyre-server suites the executor
already ran.
