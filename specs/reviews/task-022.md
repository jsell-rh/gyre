# Review: TASK-022 — Dep Graph — Cascade Testing

**Reviewer:** Verifier  
**Round:** R2  
**Commit:** `7159eece`  
**Verdict:** `complete`

---

## Findings

- [-] [process-revision-complete] **F1 (High) — Event emission untested.** `trigger_cascade_tests` emits `cascade_test_triggered` events via `state.emit_event()` (merge_processor.rs:939–946) and `report_cascade_test_result` emits `cascade_test_passed`/`cascade_test_failed` events (merge_processor.rs:1014–1024, 1069–1080). None of the 9 tests verify that any events were emitted. Events drive workspace orchestrator behavior (the spec's step 3: "Results are reported in the activity feed" and step 5: "The Workspace Orchestrator is notified of the cascade failure"). The in-memory message store is queryable in tests — at minimum, the `report_cascade_test_result_failed_creates_follow_up_task` test should verify a `cascade_test_failed` event was broadcast to the workspace, and the `trigger_cascade_tests_creates_tasks_for_dependents` test should verify a `cascade_test_triggered` event was emitted.
  > **Process revision:** Added `scripts/check-event-emission-coverage.sh` (detects tests that call emit_event-bearing functions without broadcast channel subscription). Updated implementation prompt item #47 to require broadcast channel subscribe/assert pattern for all `emit_event()` calls. Added verifier target "Untested event emission."

- [-] [process-revision-complete] **F2 (Medium) — `CascadeTestTriggered` notification completely untested.** `trigger_cascade_tests` sends `CascadeTestTriggered` notifications to workspace members (merge_processor.rs:964–978). No test sets up workspace memberships for `trigger_cascade_tests` and verifies these notifications. Only `report_cascade_test_failure_notifies_workspace_members` tests the failure notification path. Add a test for triggered-notification delivery.
  > **Process revision:** Implementation prompt item #47 now explicitly requires notification side-effect verification for EVERY notification type emitted, not just the most obvious path. The `check-event-emission-coverage.sh` script also catches this class (emit_event and notifications often co-occur in the same function).

- [-] [process-revision-complete] **F3 (Medium) — Notification body/entity_ref assertions shallow.** `report_cascade_test_failure_notifies_workspace_members` (merge_processor.rs:1419) asserts only `notification_type == CascadeTestFailed` and `title.contains("Cascade test failed")`. It does not verify:
  - `body` JSON contains the expected `task_id`, `follow_up_task_id`, `repo_id`, `failure_details` fields
  - `entity_ref` is set correctly (it should be `task.repo_id.to_string()` per line 1101)

  These fields carry the data that downstream consumers (UI, CLI, orchestrators) act on. Assert on them.
  > **Process revision:** Added "Notification payload assertion depth" sub-item to implementation prompt item #47, requiring assertion on `body` JSON fields, `entity_ref`, and `priority` — not just type and title. Added verifier target "Shallow notification assertions."

- [-] [process-revision-complete] **F4 (Medium) — Caller-scope propagation: workspace policy checked against merged repo's workspace only.** `trigger_cascade_tests` checks `require_cascade_tests` on the merged repo's workspace (merge_processor.rs:858–862). In cross-workspace scenarios (Repo A in WS-2 depends on Repo B in WS-1), the dependent repo's workspace policy is never consulted. If WS-2 has `require_cascade_tests: false`, cascade test tasks are still created in WS-2. The task is correctly scoped to the dependent's workspace (verified by `trigger_cascade_tests_resolves_dependent_workspace`), but the policy gate doesn't respect the dependent workspace's opt-out. At minimum, add a guard: skip creating the cascade test task if the dependent repo's workspace has `require_cascade_tests: false` (query `state.dependency_policies.get_for_workspace(&dep_workspace_id)`). The spec doesn't address cross-workspace explicitly, but creating tasks in a workspace that has opted out of cascade testing violates the principle of workspace-level policy control.
  > **Process revision:** Added implementation prompt item #76 "Dependent workspace policy scope" — requires per-dependent policy checking inside iteration loops. Added verifier target "Dependent workspace policy scope."

- [-] [process-revision-complete] **F5 (Low) — `report_cascade_test_result_passed_no_follow_up_task` is a weak negative test.** The test (merge_processor.rs:1347) creates a task manually, calls `report_cascade_test_result(&state, &task, true, None)`, and asserts `tasks.len() == 1`. This proves no extra task was created, but doesn't verify the positive side-effect: the `cascade_test_passed` event emission. A passing cascade test should emit the event; the test should verify it. This is partially covered by F1 but called out specifically because the test name implies comprehensive "passed" verification.
  > **Process revision:** Added "Negative test positive side-effect gap" to both implementation prompt (item #47 sub-item) and verifier prompt. The `check-event-emission-coverage.sh` script catches this specific test (it calls `report_cascade_test_result` which has emit_event but the test never subscribes).

---

## R2 — Verification of R1 Fixes

**Round:** R2  
**Commit:** `7159eece`  
**Verdict:** `complete` — all R1 findings addressed, no new findings.

### R1 Fix Verification

- [x] **F1 fix verified.** All tests that call `trigger_cascade_tests` or `report_cascade_test_result` now subscribe to the broadcast channel before the call and verify the emitted event kind and payload. 8 of 11 tests verify event emission (the remaining 3 appropriately assert no events for disabled/no-dependents/opt-out scenarios).
- [x] **F2 fix verified.** New test `trigger_cascade_tests_notifies_workspace_members` (lines 1675–1780) verifies `CascadeTestTriggered` notifications: type, title content, body JSON fields (`merged_repo`, `merge_commit_sha`, `dependent_repo`, `task_id`), `entity_ref`, and priority.
- [x] **F3 fix verified.** Both notification tests (`report_cascade_test_failure_notifies_workspace_members` and `trigger_cascade_tests_notifies_workspace_members`) now assert on body JSON fields, `entity_ref`, and priority — not just type and title.
- [x] **F4 fix verified.** Code at lines 908–922 checks dependent workspace's policy via `get_for_workspace(&dep_workspace_id)` and skips (`continue`) if `require_cascade_tests` is false. New test `trigger_cascade_tests_respects_dependent_workspace_opt_out` (lines 1785–1838) uses distinct workspace IDs (`ws-MERGED` enabled, `ws-DEPENDENT` disabled) and asserts no tasks or events are created.
- [x] **F5 fix verified.** `report_cascade_test_result_passed_no_follow_up_task` now subscribes to the broadcast channel and verifies `cascade_test_passed` event emission with correct payload fields (`event`, `task_id`, `repo_id`).

### Acceptance Criteria Sweep

All 8 acceptance criteria satisfied:
1. Post-merge hook at lines 806–813 calls `trigger_cascade_tests` after successful merge ✓
2. Task title, labels (`cascade-test`, `auto-created`), priority (High) verified in tests ✓
3. Activity events (`cascade_test_triggered`, `cascade_test_passed`, `cascade_test_failed`) emitted and tested ✓
4. Follow-up tasks created on failure with `cascade-test-failure` label, tested ✓
5. Workspace members notified via `CascadeTestFailed` notifications, event broadcast to workspace ✓
6. `require_cascade_tests: false` returns early at line 864, tested ✓
7. 11 tests cover trigger, result reporting, policy flag, cross-workspace scope, and dependent opt-out ✓
8. All 12 cascade-related tests pass ✓
