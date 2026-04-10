# Review: TASK-027 — Merge Dependencies: Atomic Group Transactional Merge

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `212f1843`  
**Verdict:** needs-revision

---

## Findings

- [ ] **F1: Premature per-member merge notifications and analytics events in the atomic group path — rolled-back merges produce irretractable "MR merged" notifications.** Inside `merge_atomic_group` (lines 573-600), each successfully merged group member immediately triggers two side effects: (1) an analytics event recording `"result": "merged"` (lines 574-586), and (2) a `notify_mr_merged` notification to the MR author (lines 589-598). If a subsequent group member fails (lines 602-643), `rollback_atomic_group` is called — the git branch is reset, MR statuses are reverted to `Open`, and queue entries are requeued. But the already-sent "MR merged" notifications and analytics events cannot be retracted. **Concrete scenario:** Group has members [MR-A, MR-B]. MR-A merges successfully → author-A receives "Your MR was merged" notification. MR-B hits a conflict → group rollback → author-A then receives "Atomic group 'bundle' failed — all members rolled back." Author-A sees contradictory notifications. The analytics store permanently records MR-A as `"result": "merged"` even though the merge was undone. **Fix:** Defer per-member notifications and analytics events until after ALL group members have merged successfully. Collect the notification/analytics data in the merge loop; emit them in a post-loop success block (after line 645).

- [ ] **F2: Over-broad notification target for atomic group failure — workspace members notified in addition to authors, contradicting spec.** The spec (merge-dependencies.md §Failure Handling) says: "Roll back all group members, requeue, **notify all authors**." The task plan (§4) says: "create a priority-3 notification for each distinct `author_agent_id` in the group." The implementation at `rollback_atomic_group` lines 738-749 collects workspace members via `list_by_workspace` and unions them with the author set: `notify_user_ids = author_ids ∪ workspace_member_ids`. This sends `AtomicGroupFailure` notifications to ALL workspace members, not just the group's authors. The spec's target is "all authors" — the workspace member union is an unauthorized expansion. **Fix:** Remove lines 738-749 (the workspace membership lookup and union). Notify only the distinct `author_agent_id` values collected in `author_ids` (lines 722-733).

## Verification Summary

- Atomic group transactional merge (`merge_atomic_group`): structurally correct — collects group members from sorted topological order, filters to queued entries, merges sequentially
- No interleaving: verified — `merge_atomic_group` processes all members in a single function call before returning to `process_next`
- Rollback (`rollback_atomic_group`): git branch reset via `state.git_ops.reset_branch` with pre-group SHA, MR statuses reverted to Open, queue entries requeued with failure reason
- `AtomicGroupFailed` event emission: correct — includes `group`, `failing_mr_id`, `failure_reason`, `member_count`, `rolled_back_count`, `member_mr_ids`
- `AtomicGroupFailure` notification priority: correct — priority 3 via `default_priority()`, matches spec
- Notification body JSON: contains all relevant fields (`group`, `failing_mr_id`, `failure_reason`, `member_count`, `rolled_back_count`)
- `MessageKind::AtomicGroupFailed`: correctly added to `gyre-common/src/message.rs` with wire name `"atomic_group_failed"`, `server_only = true`, `MessageTier::Event`, parse round-trip included in test
- `NotificationType::AtomicGroupFailure`: correctly added with `as_str`, `parse`, `default_priority(3)`, and included in exhaustive test vector
- `GitOpsPort::reset_branch`: port trait method added with doc comment; `Git2OpsAdapter` implementation correctly uses `git2::Repository::reference` (force=true) + `git2::Repository::reset` for non-bare repos; `NoopGitOps` returns `Ok(())`
- Atomic group implies dependency ordering: correct — `build_queue_dependency_graph` adds edges from earlier-enqueued to later-enqueued members in the same group
- Tests: 8 TASK-027-specific tests pass (success merge, no interleaving, rollback requeue, author notification, dependency ordering, rollback without pre-group SHA, plus two from TASK-026 that exercise group readiness skipping)
- `cargo test --all`: compiles with 1 warning (unused variable); all 31 merge_processor tests pass
