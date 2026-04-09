# Review: TASK-022 — Dep Graph — Cascade Testing

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `bfb1ef58`  
**Verdict:** `needs-revision`

---

## Findings

- [ ] **F1 (High) — Event emission untested.** `trigger_cascade_tests` emits `cascade_test_triggered` events via `state.emit_event()` (merge_processor.rs:939–946) and `report_cascade_test_result` emits `cascade_test_passed`/`cascade_test_failed` events (merge_processor.rs:1014–1024, 1069–1080). None of the 9 tests verify that any events were emitted. Events drive workspace orchestrator behavior (the spec's step 3: "Results are reported in the activity feed" and step 5: "The Workspace Orchestrator is notified of the cascade failure"). The in-memory message store is queryable in tests — at minimum, the `report_cascade_test_result_failed_creates_follow_up_task` test should verify a `cascade_test_failed` event was broadcast to the workspace, and the `trigger_cascade_tests_creates_tasks_for_dependents` test should verify a `cascade_test_triggered` event was emitted.

- [ ] **F2 (Medium) — `CascadeTestTriggered` notification completely untested.** `trigger_cascade_tests` sends `CascadeTestTriggered` notifications to workspace members (merge_processor.rs:964–978). No test sets up workspace memberships for `trigger_cascade_tests` and verifies these notifications. Only `report_cascade_test_failure_notifies_workspace_members` tests the failure notification path. Add a test for triggered-notification delivery.

- [ ] **F3 (Medium) — Notification body/entity_ref assertions shallow.** `report_cascade_test_failure_notifies_workspace_members` (merge_processor.rs:1419) asserts only `notification_type == CascadeTestFailed` and `title.contains("Cascade test failed")`. It does not verify:
  - `body` JSON contains the expected `task_id`, `follow_up_task_id`, `repo_id`, `failure_details` fields
  - `entity_ref` is set correctly (it should be `task.repo_id.to_string()` per line 1101)

  These fields carry the data that downstream consumers (UI, CLI, orchestrators) act on. Assert on them.

- [ ] **F4 (Medium) — Caller-scope propagation: workspace policy checked against merged repo's workspace only.** `trigger_cascade_tests` checks `require_cascade_tests` on the merged repo's workspace (merge_processor.rs:858–862). In cross-workspace scenarios (Repo A in WS-2 depends on Repo B in WS-1), the dependent repo's workspace policy is never consulted. If WS-2 has `require_cascade_tests: false`, cascade test tasks are still created in WS-2. The task is correctly scoped to the dependent's workspace (verified by `trigger_cascade_tests_resolves_dependent_workspace`), but the policy gate doesn't respect the dependent workspace's opt-out. At minimum, add a guard: skip creating the cascade test task if the dependent repo's workspace has `require_cascade_tests: false` (query `state.dependency_policies.get_for_workspace(&dep_workspace_id)`). The spec doesn't address cross-workspace explicitly, but creating tasks in a workspace that has opted out of cascade testing violates the principle of workspace-level policy control.

- [ ] **F5 (Low) — `report_cascade_test_result_passed_no_follow_up_task` is a weak negative test.** The test (merge_processor.rs:1347) creates a task manually, calls `report_cascade_test_result(&state, &task, true, None)`, and asserts `tasks.len() == 1`. This proves no extra task was created, but doesn't verify the positive side-effect: the `cascade_test_passed` event emission. A passing cascade test should emit the event; the test should verify it. This is partially covered by F1 but called out specifically because the test name implies comprehensive "passed" verification.
