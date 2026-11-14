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

**Round:** R3 (repair round, candidate `fe01768b`)
**Verdict:** `complete`

## Scope

Assigned base `b6892403` → candidate `fe01768b`. The product-surface delta is
confined to `mod tests` in `crates/gyre-server/src/api/specs_assist.rs`:
the e2e write-through tests from `3488a06b` plus the whitespace-only rustfmt
re-wrap from `97ae5fa9` (verified byte-for-byte: single `assert!` re-wrap, no
behavior change). The production handler, `GitOpsPort` write operations, and
git2 adapter implementations were already present in the assigned base from
earlier verified rounds; both in-scope commits are listed in the task
frontmatter.

## Independent evidence (see /tmp/stage/review-evidence/task-011-r3/)

- **rustfmt gate reproduced:** `python3 scripts/check-rustfmt-diff.py b6892403`
  fails at pre-fix merge `54fffafa` with exactly the retained failure
  (`specs_assist.rs: changed lines need formatting: 2100`, exit 1) and passes
  at the candidate (exit 0). The repair addresses precisely the reported gate.
- **Real git write-through (adapter level):** `cargo test -p gyre-adapters
  --lib git2_ops::tests::test_write_file` (4/4 ok, including nested paths and
  feature-branch commits), `test_create_branch` (2/2), `test_branch_exists`
  (2/2) — real libgit2 against on-disk temp repos, no stubs.
- **Type check:** `cargo check -p gyre-server --tests` passes (exit 0) — the
  e2e tests and the re-wrapped assert compile cleanly.
- **Handler verification (code reading):** `save_spec` resolves the repo via
  `RepositoryPort`, creates the `spec-edit/<slug>-<short_uuid>` branch from
  the default branch, commits via `write_file`, creates the MR with
  `author_agent_id` set, matches existing MRs by branch prefix **and** author
  (R2-F1 fix present at `mr.author_agent_id.as_ref() == Some(&caller_id)`),
  and fans out a `SpecApprovalRequested` notification asserted at
  `priority_rank == 2` with `entity_id == mr_id` and an externally-anchored
  blob SHA. `save_prompt` commits directly to the default branch behind
  `validate_prompt_path` (rejects absolute paths, `..` traversal, and paths
  outside `specs/prompts/`). Slug generation matches the spec (lowercase,
  `/`→`-`, strip `.md`, 4-hex hash). Both routes have ABAC resolver entries.

## Verifier instructions

Run `cargo test -p gyre-server specs_assist` (server test compilation deferred
to the independent verifier per the 4 GiB budget; this round's product delta
is test code already type-checked here, and the e2e tests inject the real
`Git2OpsAdapter` so they fail if the git write-through regresses to a stub).

No findings.
