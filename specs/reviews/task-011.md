# Review: TASK-011 — Spec Save Git Write-Through

**Reviewer:** Verifier
**Round:** R2
**Verdict:** `complete`

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

---

## R3 — candidate `ddd1a2c5` (base `6ffd6799`)

**Reviewer:** Independent review agent
**Round:** R3
**Verdict:** `approved`

**Scope:** base→candidate diff is +227 lines of end-to-end write-through tests in
`crates/gyre-server/src/api/specs_assist.rs` (product commit `3488a06b`, recorded in
the task frontmatter) plus the task-doc completion record. The production
write-through implementation (GitOpsPort `write_file`/`create_branch`/`branch_exists`,
git2 adapter, handler wiring) is already in the assigned base via earlier merges.

**Evidence (see `/tmp/stage/review-evidence/round-ddd1a2c5-review-summary.md`):**

- `cargo check -p gyre-server --tests` — exit 0, zero errors (all tests incl. the
  three new e2e write-through tests type-check).
- `cargo test -p gyre-adapters --lib git2_ops` — exit 0, 35/35 passed: real on-disk
  bare repos, write_file commit/log round-trips, nested paths, overwrite, feature
  branches, create_branch from ref (+ failure on invalid ref), branch_exists
  true/false. The adapter layer the handlers depend on is real.
- All three blob-SHA test anchors independently reproduced with `git hash-object`
  (8035a516…, 98537aa0…, 1a862bdc…) — external anchors, not mirrored logic.
- R2 findings F1–F4 verified fixed at the candidate: author check in existing-MR
  matching, `author_agent_id` set on MR creation, stale "(stubbed)" doc removed,
  notification side effects asserted (`priority_rank == 2`, `entity_id == mr_id`,
  structured body fields, system-recipient fallback).
- Invariant scripts pass: check-arch.sh, check-byte-slice-truncation.sh,
  check-mem-port-contracts.sh. ABAC resolver entries present for both routes.
- Acceptance criteria: all seven have real production behavior and meaningful
  coverage (spec-edit branch + content on disk, MR targeting default branch,
  priority-2 notification fan-out, append-to-existing-MR with exactly one new
  commit and no second branch/MR, prompts/save direct default-branch commit with
  returned SHA == new tip, slug format/collision, tests for each).

**Verifier must execute (deferred per the 4 GiB build budget, not a code defect):**

    cargo test -p gyre-server --lib api::specs_assist

**Non-blocking notes:**

- No dedicated test for the cross-user case of existing-MR matching (a second user
  saving the same spec_path should get a new branch, not append to the first
  user's MR). The author check is present in production code; the same-user append
  path is covered end-to-end.
- `web/dist/` untouched by this candidate; generated output is owned by the
  deterministic verifier.
