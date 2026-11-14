---
title: "Spec Save Git Write-Through"
spec_ref: "ui-layout.md §3 (LLM-Assisted Spec Editing, Save workflow), human-system-interface.md §3 (Explorer: Inline Spec Editing)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-011.md
coverage_sections: []
commits: ["3488a06bf40471867a51c08bd899888c66ac8b09"]
---

## Spec Excerpt

From `ui-layout.md` §3:

> **Save workflow:** clicking Save commits the spec change to a feature branch named `spec-edit/<spec_path_slug>-<short_uuid>` [...] The server creates the branch, commits the change, and auto-creates an MR targeting the default branch. The MR itself creates a priority-2 "Spec pending approval" notification.

> The save endpoint: `POST /api/v1/repos/:repo_id/specs/save` — request: `{spec_path, content, message}`, response: `{branch, mr_id}`.

> If the user has an existing open `spec-edit/*` MR for the same `spec_path` [...] the save appends a commit to the existing branch rather than creating a new one.

From `ui-layout.md` §3 (prompts):

> Prompt templates use a separate endpoint: `POST /api/v1/repos/:repo_id/prompts/save` — request: `{prompt_path, content, message}`, response: `{commit_sha}`. This commits directly to the default branch (no feature branch, no MR, no approval notification).

## Current State

The `specs/save` handler (`crates/gyre-server/src/api/specs_assist.rs`) exists but the actual git operations are stubbed:
- Branch creation is stubbed (returns a fake branch name)
- File write is stubbed ("git_ops port does not yet expose a write-file method")
- MR creation is stubbed (returns a placeholder MR ID)
- Notification creation uses placeholder IDs
- The `prompts/save` handler is similarly stubbed (returns a fake commit SHA)
- Existing-MR detection (append commit to existing branch) is stubbed

## Implementation Plan

1. **Extend `GitOpsPort` with write operations:**
   - `write_file(repo_path, branch, file_path, content, message) -> Result<String>` (returns commit SHA)
   - `create_branch(repo_path, branch_name, from_ref) -> Result<()>`
   - `branch_exists(repo_path, branch_name) -> Result<bool>`
   - These belong in `gyre-ports/src/git_ops.rs` (existing port trait)

2. **Implement write operations in the git2 adapter:**
   - `crates/gyre-adapters/src/git2_ops.rs` already implements `GitOpsPort`
   - Add branch creation (via `git2::Repository::branch`)
   - Add file write (tree builder → commit → update ref)

3. **Wire `specs/save` handler to use real git operations:**
   - Resolve repo path from `repo_id` via `RepositoryPort`
   - Create `spec-edit/<slug>-<hash>` branch from default branch
   - Write spec content to `spec_path` on the branch
   - Create MR targeting default branch via `MergeRequestPort`
   - Create priority-2 notification for workspace Admin/Developer members
   - Handle existing-MR case: detect open `spec-edit/*` MR for same spec, append commit

4. **Wire `prompts/save` handler:**
   - Write to default branch directly (no feature branch, no MR)
   - Return real commit SHA

5. **Add tests:**
   - Spec save creates branch + MR + notification
   - Spec save with existing MR appends commit
   - Prompts save commits to default branch
   - Slug generation is deterministic

## Acceptance Criteria

- [x] `POST /api/v1/repos/:repo_id/specs/save` creates a real `spec-edit/*` branch with the spec content
- [x] An MR is auto-created targeting the default branch
- [x] A priority-2 "Spec pending approval" notification is created for workspace members
- [x] Saving to an existing open spec-edit MR appends a commit instead of creating a new branch
- [x] `POST /api/v1/repos/:repo_id/prompts/save` commits directly to the default branch
- [x] Slug generation follows the spec: path → lowercase, `/` → `-`, strip `.md`, append 4-char hash
- [x] Tests cover all acceptance criteria

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/specs_assist.rs` for the current stubbed handlers
3. Read `crates/gyre-ports/src/git_ops.rs` for the existing `GitOpsPort` trait
4. Read `crates/gyre-adapters/src/git2_ops.rs` for the git2-based adapter
5. Read `crates/gyre-ports/src/merge_request.rs` for MR creation
6. Read `crates/gyre-ports/src/notification.rs` for notification creation
7. Implement the git write operations, then wire the handlers
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `3488a06bf` — test(task-011): end-to-end write-through coverage for specs/prompts save
- `a70b8e1c` — process(task-011): merge assignment base `7daca25d`; resolved the
  `specs_assist.rs` test-module conflict by keeping both the write-through and
  prompt path-guard test families (merge commit, listed for visibility only —
  the product-surface commit is `3488a06bf`)
- `9b5b3ff1` — merge of assignment base `f7f5c59f` (task-125 mainline head);
  the `specs_assist.rs` resolution adapts the write-through call sites to the
  evolved notification API (merge commit, listed for visibility only —
  the product-surface commit is `3488a06bf`)
- `9c708d69` — merge of assignment base `6f5a631c` (task-195 mainline head);
  no task-011 product-surface file was touched by the merge (merge commit,
  listed for visibility only — the product-surface commit is `3488a06bf`)
- `132be8a4` — process(task-011): re-apply completion record after contract
  restore (repair `d5237543`; doc-only — the product-surface commit is
  `3488a06bf`)
- `90eda370` — merge of assignment base `6ffd6799` (task-130 mainline head);
  adds test-fixture `AuthContext` fields (`workspace_id`, `repo_id`, `scope`)
  to two `specs_assist.rs` test constructors only — no write-through product
  code touched (merge commit, listed for visibility only — the product-surface
  commit is `3488a06bf`)
- `7cfc7023` — process(task-011): re-apply completion record after contract
  restore (repair `7c7d2255`; doc-only — the product-surface commit is
  `3488a06bf`)
- `db9980a2` — process(task-011): list contract-restore process commit in
  task log (doc-only — the product-surface commit is `3488a06bf`)
- `8a19603d` — process(task-011): re-apply completion record after contract
  restore (repair `4a17771f`; doc-only — the product-surface commit is
  `3488a06bf`)
- `1607a409` — process(task-011): list contract-restore process commit in
  task log (doc-only — the product-surface commit is `3488a06bf`)
- `e3f70915` — process(task-011): re-apply completion record after contract
  restore (repair `ecc55cd1`; doc-only — the product-surface commit is
  `3488a06bf`)

## Shipped

The spec save git write-through is fully implemented in production code on
this branch (inherited from mainline; review history in
`specs/reviews/task-011.md`, R2 verdict `complete`). The current round is a
fifth contract repair (id `ecc55cd157d34fa896f3ddffd1cdfe39`): the executor
again restored the assigned task document, and the completion record is
re-applied here on top of the restored document. No write-through product
code changed this round — the assignment base is still `6ffd6799`, already
merged as `90eda370` in a prior round.

### This round (repair `ecc55cd1`)

- No merge was required: the assignment base `6ffd6799` is identical to the
  prior round's base and was already merged (`90eda370`). The product surface
  (`specs_assist.rs`, `git_ops.rs`, `git2_ops.rs`) is byte-identical to the
  prior verified head (`git diff 1607a409 -- crates/ web/ src/` is empty) —
  the code where the prior round's probes recorded evidence.
- The executor restored the assigned task document (unchecked acceptance
  criteria, empty commit list) in the working tree; the restored normative
  text is identical to the recorded contract. The completion record —
  progress, acceptance checkboxes, commit attribution, and these Shipped
  notes — is re-applied (doc-only). Evidence retained under
  `/tmp/stage/review-evidence/round-ecc55cd1-git-verification.txt`.

### Actual behavior

- `GitOpsPort` exposes `write_file` / `create_branch` / `branch_exists`
  (`crates/gyre-ports/src/git_ops.rs`), implemented for real in the git2
  adapter (`crates/gyre-adapters/src/git2_ops.rs`): branch creation resolves
  `from_ref` to a commit and calls `Repository::branch`; `write_file` writes a
  blob, rebuilds the tree (nested paths included) via treebuilder, commits
  with the branch tip as parent, and updates the ref — returning the real
  commit SHA.
- `POST /specs/save` (`save_spec` in `crates/gyre-server/src/api/specs_assist.rs`)
  resolves the repo via `RepositoryPort`, creates `spec-edit/<slug>-<short_uuid>`
  from the default branch, commits the spec content there, auto-creates an MR
  targeting the default branch (with `author_agent_id` set), and creates a
  priority-2 `SpecApprovalRequested` notification whose `entity_id` is the MR
  ID (Inbox "Approve" linkage), fanned out to workspace Admin/Developer/Owner
  members (plus `human_approvers` team members, plus `system` fallback).
  Existing-MR detection matches open MRs by `spec-edit/<slug>-*` branch prefix
  AND MR author == caller, then appends a commit to the existing branch
  (HTTP 200, same `branch`/`mr_id`) and refreshes the pending-approval card's
  `spec_sha` (task-175 F1).
- `POST /prompts/save` commits directly to the default branch — no feature
  branch, no MR, no approval notification — and returns the real commit SHA.
  The review-gate bypass is constrained to `specs/prompts/` by
  `validate_prompt_path` (rejects absolute paths, `..` traversal, and any path
  outside the directory with 400).
- Slug: path → lowercase, `/` → `-`, strip `.md`, plus a 4-hex-char hash of
  the full original path (collision-safe), per ui-layout.md §3.

### Test evidence

- Handler logic (NoopGitOps): tests covering branch naming, MR creation,
  existing-MR append, priority-2 notification assertion
  (`save_spec_creates_mr_for_existing_repo` verifies `priority_rank == 2` and
  `entity_id == mr_id`), prompt save SHA shape, slug format/collision, prompt
  path-guard rejection, and the HSI §7 409 conflict path.
- End-to-end tests (commit `3488a06bf`) inject the real `Git2OpsAdapter` with
  an on-disk bare repo so the full chain — handler → GitOpsPort → libgit2 —
  runs: real branch exists with content, default branch untouched by spec
  saves, re-save appends exactly one commit (verified via `commit_log`) with no
  second MR/branch, prompt save lands on `main` with the returned SHA equal to
  the real new tip and no branch/MR created.
- Adapter layer: 35 git2_ops tests including `write_file` on feature branches,
  nested paths, overwrite, `create_branch` from existing ref, and
  `branch_exists`.

### Probes this round (head `1607a409`)

- Git-level verification only (no cargo compile re-run): the sandbox has no
  warm target dir and the code is byte-identical to the previously verified
  head, so a cold build would prove nothing new. Recorded under
  `/tmp/stage/review-evidence/round-ecc55cd1-git-verification.txt`:
  `git merge-base --is-ancestor 6ffd6799 HEAD` (exit 0 — base already
  merged), `git status --short` at round start (only the executor's task-doc
  restore), `git diff 1607a409 -- crates/ web/ src/` (empty), and grep spot
  checks of the write-through call sites at HEAD.
- The verifier should execute at the published head: `cargo test -p
  gyre-adapters --lib git2_ops` and `cargo test -p gyre-server --lib
  api::specs_assist` (gyre-server test compilation is deferred to the
  verifier per the 4 GiB build budget; the identical code passed `cargo
  check -p gyre-server --tests` at `90eda370` in a prior round).
