---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: needs-revision
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["d2f1d35bfa8f3facb2d09b49f95d880d35e9a5f2", "7be4bb97763fb040b28a79375a434f7a3ed9c788", "cdc152f2bc9ab133de06b083b5eb780a519d00d9", "e500a1fd4ddb6d87dc9f4674ae47df3c9f96f6cf", "21a6f6f78acc8a5e773932df96bf733fc97cd838", "f7522000225c20101f2c6a7b21b0e10393f8e74a", "5fbc16b1f8ccc65479343718cc7213d036487ab8"]
---

## Spec Excerpt

From `source-control.md` §4:

> When the target branch moves (because another agent's MR merged), jj automatically rebases the agent's in-progress work. With git, the agent must manually `git fetch && git rebase`, handle conflicts, and continue. In a system with 20+ agents pushing concurrently, the baseline moves constantly. jj handles this transparently.

Current state: The speculative merge system (`speculative_merge.rs`) detects conflicts when the target branch moves, but it does not trigger an automatic `jj rebase` on the agent's working branch. Conflict detection is passive — the agent must manually handle the rebase.

## Implementation Plan

1. **Rebase trigger in merge processor:**
   - After a successful merge (MR lands on default branch), identify all other in-flight agents targeting the same branch
   - For each affected agent, trigger a `jj rebase` operation on their working change
   - This should run in `gyre-server/src/merge_processor.rs` after the merge commit

2. **jj rebase adapter (`gyre-adapters/src/jj_ops.rs`):**
   - Add `jj_rebase(repo_path, revision, destination)` function
   - Execute `jj rebase -r <revision> -d <destination>` in the agent's worktree
   - Capture and return rebase result (success, conflict, or error)

3. **Conflict handling:**
   - If rebase produces conflicts, emit a `SpeculativeConflict` event
   - Set the conflict state on the agent's MergeQueueEntry
   - Notify the agent via WebSocket message (MessageKind::Escalation or similar)
   - Agent can continue working on non-conflicting files (jj conflict-as-state model)

4. **Agent notification:**
   - After successful rebase, send a WebSocket notification to the agent informing it the baseline moved
   - Include the new base commit SHA in the notification
   - After conflict rebase, send conflict details with affected file list

5. **Safeguards:**
   - Only rebase agents that are in Active status (not Dead/Completed)
   - Skip rebase if the agent's worktree path doesn't exist
   - Log all rebase operations for audit trail
   - Rate-limit rebases: if target moves multiple times in quick succession, batch into one rebase

## Acceptance Criteria

- [x] `jj_rebase` function added to jj_ops adapter
- [x] After MR merge, in-flight agents on same target branch are automatically rebased
- [x] Conflicts from rebase are surfaced as state (not errors)
- [x] Agents notified via WebSocket of baseline movement
- [x] Rebase skipped for dead/completed agents
- [x] Rebase operations logged
- [x] `cargo test --all` passes

## Spec-vs-Plan Divergence (review F5)

The plan said `jj rebase -r <revision> -d <destination>`; the spec (§4) says
the agent's "in-progress work" — the whole stack — moves onto the new base.
`-r @` would rebase only the working-copy commit and abandon its descendants
onto the old parent; the adapter therefore runs `jj rebase -b @ -d <dest>`,
which rebases the branch containing `@` (the entire in-flight stack).
Verified empirically against jj 0.39.0 (see
`jj_rebase_clean_after_target_moves` in `crates/gyre-adapters/src/jj_ops.rs`).

## Agent Instructions

Read `specs/system/source-control.md` §4 for the full spec. The jj adapter is in `gyre-adapters/src/jj_ops.rs` — add `jj_rebase` alongside existing `jj_new`, `jj_undo`, etc. The merge processor is in `gyre-server/src/merge_processor.rs` — the post-merge hook is where rebase should be triggered. The speculative merge system is in `gyre-server/src/speculative_merge.rs` for conflict detection patterns. WebSocket messaging is in `gyre-server/src/ws.rs`. Agent worktree paths are tracked in `agent_tracking.rs`.
