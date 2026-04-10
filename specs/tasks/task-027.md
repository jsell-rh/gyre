# TASK-027: Merge Dependencies — Atomic Group Transactional Merge

**Spec reference:** `merge-dependencies.md` §Atomic Groups, §Atomic Group Rules, §Failure Handling  
**Depends on:** TASK-026 (topological sort processes groups in correct order)  
**Progress:** `complete`  
**Review:** [`specs/reviews/task-027.md`](../reviews/task-027.md)

## Spec Excerpt

From `merge-dependencies.md` §Atomic Group Rules:

> 1. All members must have all gates passed before any member merges.
> 2. The merge processor locks the queue and merges all members in dependency order with no interleaving.
> 3. If any member fails to merge (conflict), the entire group is rolled back and requeued.
> 4. Atomic groups imply dependency ordering among members (if not already declared).

From §Failure Handling:

> | Atomic group member fails merge | Roll back all group members, requeue, notify all authors |

## Current State

The merge processor (`crates/gyre-server/src/merge_processor.rs`) has:
- `atomic_group_ready()` (lines 47–71) — checks if all members have gates passed and deps satisfied
- Integration in `process_next()` that checks readiness and skips if group not ready

**Missing:**
- No lock-and-merge-all: the processor merges one entry per cycle, so group members merge across separate cycles with potential interleaving
- No rollback: if a group member fails mid-merge, already-merged members are not reverted
- No multi-member sequential merge in a single cycle
- No notification to all group member authors on failure

## Implementation Plan

1. **Detect atomic group readiness in `process_next()`:**
   - When the topological sort (TASK-026) selects an entry that is part of an atomic group:
     - Collect all group members from the queue
     - Check `atomic_group_ready()` — all members must have gates passed and deps satisfied
     - If not all ready, skip the entire group (existing behavior)

2. **Lock-and-merge-all:**
   - When all group members are ready, merge them sequentially in dependency order:
     - Sort group members by their topological position (from TASK-026)
     - For each member: run the existing merge logic (git merge, attestation, status update)
     - Do NOT allow other queue entries to interleave (the loop processes all members before returning)

3. **Rollback on failure:**
   - If any member fails to merge (git conflict, constraint failure):
     - For already-merged members in this cycle: reset the target branch to the pre-group commit via `git reset --hard <pre_group_sha>`
     - Requeue all group members (set status back to `Queued`)
     - Record the failure reason on each member's queue entry
   - **Implementation detail:** Before starting the group merge, record the target branch HEAD SHA. On failure, reset to that SHA.

4. **Notify all authors:**
   - On group failure, create a priority-3 notification for each distinct `author_agent_id` in the group
   - Notification type: `AtomicGroupFailure` with group name, failing member, and reason
   - Also emit a `MessageKind::AtomicGroupFailed` Event-tier message to the workspace

5. **Add tests:**
   - Successful atomic group: all members merge in one cycle
   - Failed atomic group: second member conflicts → first is rolled back, all requeued
   - Group with unsatisfied deps: entire group skipped
   - Author notification on failure

## Acceptance Criteria

- [ ] Atomic group members merge sequentially in dependency order within a single processor cycle
- [ ] No interleaving: other queue entries do not merge between group members
- [ ] If any member fails, all already-merged members are rolled back and the group is requeued
- [ ] All distinct authors in the group receive failure notifications
- [ ] Atomic group implies dependency ordering among members
- [ ] Tests cover successful merge, rollback on failure, and notification
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `merge-dependencies.md` §Atomic Groups, §Atomic Group Rules, §Failure Handling
3. Read `crates/gyre-server/src/merge_processor.rs` — focus on `atomic_group_ready()` and the merge logic in `process_next()`
4. Read `crates/gyre-adapters/src/git2_ops.rs` for git operations (branch reset, HEAD lookup)
5. Extend the merge processor to handle atomic groups as a transactional unit
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(merge): implement atomic group transactional merge (TASK-027)`
