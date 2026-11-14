---
title: "Spec Save Git Write-Through"
spec_ref: "ui-layout.md §3 (LLM-Assisted Spec Editing, Save workflow), human-system-interface.md §3 (Explorer: Inline Spec Editing)"
depends_on: []
progress: complete
review: specs/reviews/task-011.md
coverage_sections: []
commits: ["97ae5fa923b479420710a28839be49189220e428", "3488a06bf40471867a51c08bd899888c66ac8b09"]
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

- [ ] `POST /api/v1/repos/:repo_id/specs/save` creates a real `spec-edit/*` branch with the spec content
- [ ] An MR is auto-created targeting the default branch
- [ ] A priority-2 "Spec pending approval" notification is created for workspace members
- [ ] Saving to an existing open spec-edit MR appends a commit instead of creating a new branch
- [ ] `POST /api/v1/repos/:repo_id/prompts/save` commits directly to the default branch
- [ ] Slug generation follows the spec: path → lowercase, `/` → `-`, strip `.md`, append 4-char hash
- [ ] Tests cover all acceptance criteria

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

_(none yet)_

## Shipped

The spec save git write-through is implemented in production code on this
branch. Product-surface commits (review-scoping list, frontmatter above):
`3488a06b` (e2e write-through coverage) and `97ae5fa9` (this round's rustfmt
repair). Review history: `specs/reviews/task-011.md`, R2 verdict `complete`
with F1–F4 all process-revision-complete. The `## Git Commits` body section
above is kept at the assigned baseline: it is protected contract text.

### This round (repair `97ae5fa9`)

- The retained verification run (`bash /tmp/stage/checks.sh`, candidate
  `ddd1a2c`) failed on exactly one gate: `rustfmt:
  crates/gyre-server/src/api/specs_assist.rs: changed lines need formatting:
  2100`. Everything else passed (clippy changed-lines clean, arch/hierarchy
  lints, all invariant scripts, web build).
- Fix: re-wrapped the one-line
  `assert!(state.merge_requests.list_by_repo(...).is_empty())` in
  `save_prompt_commits_directly_to_default_branch` into the edition-2021
  rustfmt shape. Whitespace-only; no behavior change.
- Local proof: `python3 scripts/check-rustfmt-diff.py b6892403` → "changed
  lines clean" (exit 0), reproducing the failing gate verbatim before and
  after the fix. Evidence under `/tmp/stage/review-evidence/`.
- No merge was required: the assignment base `b6892403` was already merged
  (`54fffafa`), and the product surface is otherwise byte-identical to the
  previously verified candidate `ddd1a2c` (the only code delta this round
  is the re-wrapped assert).

### Actual behavior

- `GitOpsPort` exposes `write_file` / `create_branch` / `branch_exists`
  (`crates/gyre-ports/src/git_ops.rs`), implemented for real in the git2
  adapter (`crates/gyre-adapters/src/git2_ops.rs`): branch creation resolves
  `from_ref` to a commit and calls `Repository::branch`; `write_file` writes
  a blob, rebuilds the tree (nested paths included) via treebuilder, commits
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
  AND MR author == caller (R2-F1), then appends a commit to the existing
  branch (HTTP 200, same `branch`/`mr_id`) and refreshes the pending-approval
  card's `spec_sha` (task-175 F1).
- `POST /prompts/save` commits directly to the default branch — no feature
  branch, no MR, no approval notification — and returns the real commit SHA.
  The review-gate bypass is constrained to `specs/prompts/` by
  `validate_prompt_path` (rejects absolute paths, `..` traversal, and any path
  outside the directory with 400).
- Slug: path → lowercase, `/` → `-`, strip `.md`, plus a 4-hex-char hash of
  the full original path (collision-safe), per ui-layout.md §3.

### Test evidence

- Handler logic (NoopGitOps): tests covering branch naming, MR creation,
  existing-MR append with author matching, priority-2 notification assertion
  (`save_spec_creates_mr_for_existing_repo` verifies `priority_rank == 2` and
  `entity_id == mr_id`, R2-F4), prompt save SHA shape, slug format/collision,
  prompt path-guard rejection, and the HSI §7 409 conflict path.
- Real git write-through (e2e, real git2 adapter on a temp repo, commit
  `3488a06b`): `save_spec_creates_branch_mr_and_commits_content`, existing-MR
  append appends a real commit to the existing branch, prompt save commits to
  the default branch and returns the real new tip SHA (no feature branch, no
  MR — the assertion re-wrapped this round).
- Verifier should run: `cargo test -p gyre-server specs_assist` (server test
  compilation is deferred to the independent verifier per the 4 GiB budget;
  this round's delta is whitespace-only inside one test assertion, and
  rustfmt's successful parse plus the changed-lines gate prove syntax and
  formatting validity).
