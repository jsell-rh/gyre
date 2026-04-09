# TASK-021 Review — Version Drift Tracking & Stale Dependencies

## R1 — needs-revision, 2 findings

### Findings

- [-] [process-revision-complete] **F1: Push-time dependency detection does not set status to `Stale` when drift exceeds `max_version_drift`.**
  The task's implementation plan (step 1) explicitly states: "If drift exceeds workspace policy
  `max_version_drift`, set `DependencyEdge.status = Stale`." The push handler at
  `git_http.rs:1835-1868` computes `version_drift` and saves the edge, but never reads the
  workspace policy or checks `max_version_drift`. The edge is always saved with
  `status: Active` (the default from `DependencyEdge::new`), regardless of drift magnitude.
  The background job (`dep_staleness.rs:99-112`) correctly performs this check daily, but
  the task plan specifies push-time detection. Between a push and the next daily job run (up
  to 24 hours), `GET /api/v1/dependencies/stale` returns incomplete results — a dependency
  with drift of 10 versions would appear Active. The fix: after computing `version_drift`
  in the push handler, look up the workspace policy and set `edge.status = Stale` if
  `drift > policy.max_version_drift`.
  Process guard: implementation prompt items 10 (task traceability — acceptance criteria sweep)
  and 54 (spec-excerpt schema field completeness). The acceptance criterion explicitly states
  "Dependencies exceeding `max_version_drift` set to `Stale` status."

- [-] [process-revision-complete] **F2: Time-based staleness is ephemeral — `last_verified_at` is unconditionally reset, causing stale edges to revert on the next job cycle.**
  `dep_staleness.rs:85` unconditionally sets `updated_edge.last_verified_at = now` for every
  edge, regardless of whether version resolution succeeded or the edge is stale. The
  time-based staleness check at line 117 compares `now - edge.last_verified_at > threshold_secs`.
  Sequence: (1) Job runs when `edge.last_verified_at` is 31 days old. (2) Check triggers
  (`31 days > 30 day threshold`), edge marked Stale. (3) `last_verified_at` is set to `now`
  and saved (line 183). (4) Next day, job runs again: `now - last_verified_at ≈ 24 hours`.
  (5) `24 hours < 30 day threshold`, so `is_stale = false`. (6) Line 177:
  `!is_stale && updated_edge.status == Stale` → status reverts to Active. The edge is
  Stale for exactly one day, then automatically reverts — even though the underlying
  condition (dependency not updated) has not changed. No test verifies persistence across
  multiple job runs. The `test_time_based_staleness_detection` test (line 240) runs the
  job once and asserts Stale, but a second `run_once` would revert the edge to Active.
  Fix: do not update `last_verified_at` unconditionally. Either (a) only update it when
  version resolution succeeds AND the version has changed, or (b) use a separate field
  (e.g., `last_checked_at`) for the job's housekeeping timestamp, keeping `last_verified_at`
  to track when the dependency version was last confirmed current.
  Process guard: implementation prompt item 71 (multi-cycle testing for periodic jobs — new),
  verifier prompt addition (single-cycle test detection for periodic jobs).
