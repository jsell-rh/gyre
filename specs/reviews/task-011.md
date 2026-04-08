# Review: TASK-011 — Spec Save Git Write-Through

**Reviewer:** Verifier
**Round:** R1
**Verdict:** `needs-revision`

---

## Findings

- [-] **[process-revision-complete] F1: Existing-MR detection omits required author check (spec violation)**
  `specs_assist.rs:216-218` — The spec (ui-layout.md §3, line 338) explicitly requires existing-MR matching to check "the MR author is the current user": *"matched by querying open MRs where the branch matches `spec-edit/<spec_path_slug>-*` and the MR author is the current user."* The implementation only checks branch prefix and open status:
  ```rust
  let existing_mr = all_mrs
      .into_iter()
      .find(|mr| mr.status == MrStatus::Open && mr.source_branch.starts_with(&branch_prefix));
  ```
  This allows User A to append commits to User B's spec-edit branch for the same spec path. The handler must extract `AuthenticatedAgent` (or equivalent caller identity) and filter MRs by `author_agent_id` matching the caller.

- [-] **[process-revision-complete] F2: MR `author_agent_id` not set on creation**
  `specs_assist.rs:269-278` — The `MergeRequest::new(...)` call leaves `author_agent_id: None` (the default). The handler does not extract `AuthenticatedAgent`, so it has no caller identity to record. Even if the author check from F1 were added, there would be no stored author to match against. The handler must extract the caller and set `mr.author_agent_id = Some(caller_id)`.

- [-] **[process-revision-complete] F3: Stale "(stubbed)" in module doc comment**
  `specs_assist.rs:5` — The module-level doc comment reads:
  ```rust
  //! POST /api/v1/repos/:id/prompts/save   — direct commit to default branch (stubbed)
  ```
  The `prompts/save` handler (lines 327-354) is now fully implemented with real git operations. The "(stubbed)" label is stale.

- [-] **[process-revision-complete] F4: Tests do not verify notification creation (compound acceptance criterion partially satisfied)**
  Acceptance criteria state: *"A priority-2 'Spec pending approval' notification is created for workspace members"* and *"Tests cover all acceptance criteria."* The test `save_spec_creates_mr_for_existing_repo` (line 518) verifies the response (branch, mr_id) but never queries the notification repository to assert that a notification was created with `NotificationType::SpecPendingApproval`, priority 2, and `entity_ref` matching the MR ID. Given that the notification create call uses `let _ = ...` (line 306), silently swallowing errors, a regression in notification creation would be invisible to tests.
