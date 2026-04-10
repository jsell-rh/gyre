# Review: TASK-027 — Merge Dependencies: Atomic Group Transactional Merge

**Reviewer:** Verifier  
**Round:** R2  
**Commit:** `0674ff98`  
**Verdict:** needs-revision

---

## R1 Findings (resolved)

- [x] [process-revision-complete] **F1: Premature per-member merge notifications and analytics events in the atomic group path — rolled-back merges produce irretractable "MR merged" notifications.** Inside `merge_atomic_group` (lines 573-600), each successfully merged group member immediately triggers two side effects: (1) an analytics event recording `"result": "merged"` (lines 574-586), and (2) a `notify_mr_merged` notification to the MR author (lines 589-598). If a subsequent group member fails (lines 602-643), `rollback_atomic_group` is called — the git branch is reset, MR statuses are reverted to `Open`, and queue entries are requeued. But the already-sent "MR merged" notifications and analytics events cannot be retracted. **Concrete scenario:** Group has members [MR-A, MR-B]. MR-A merges successfully → author-A receives "Your MR was merged" notification. MR-B hits a conflict → group rollback → author-A then receives "Atomic group 'bundle' failed — all members rolled back." Author-A sees contradictory notifications. The analytics store permanently records MR-A as `"result": "merged"` even though the merge was undone. **Fix:** Defer per-member notifications and analytics events until after ALL group members have merged successfully. Collect the notification/analytics data in the merge loop; emit them in a post-loop success block (after line 645). **R2:** Fixed correctly — side effects moved to post-loop block (lines 620-648).

- [x] [process-revision-complete] **F2: Over-broad notification target for atomic group failure — workspace members notified in addition to authors, contradicting spec.** The spec (merge-dependencies.md §Failure Handling) says: "Roll back all group members, requeue, **notify all authors**." The task plan (§4) says: "create a priority-3 notification for each distinct `author_agent_id` in the group." The implementation at `rollback_atomic_group` lines 738-749 collects workspace members via `list_by_workspace` and unions them with the author set: `notify_user_ids = author_ids ∪ workspace_member_ids`. This sends `AtomicGroupFailure` notifications to ALL workspace members, not just the group's authors. The spec's target is "all authors" — the workspace member union is an unauthorized expansion. **Fix:** Remove lines 738-749 (the workspace membership lookup and union). Notify only the distinct `author_agent_id` values collected in `author_ids` (lines 722-733). **R2:** Fixed correctly — `list_by_workspace` removed, only `author_ids` iterated. Test updated to assert workspace member does NOT receive notification.

## R2 Findings

- [-] [process-revision-complete] **F3: Deferred side-effects (F1 fix) have zero test coverage on the success path — no test asserts that per-member analytics events or author notifications are emitted after a successful atomic group merge.** The F1 fix moved analytics and notification emissions from inside the merge loop to a post-loop success block (lines 620-648). This is a new code path. The only test that exercises the success path, `atomic_group_all_members_merge_in_one_cycle` (line 3364), asserts on MR statuses (Merged), queue entry statuses (Merged), and the absence of `AtomicGroupFailed` events — but has zero assertions on the deferred side effects. It does not: (a) query `state.notifications.list_for_user(...)` for either `agent-1` or `agent-2` to verify `notify_mr_merged` was called, or (b) query the analytics repository for `merge_queue.processed` events. If someone accidentally deleted lines 620-648, all tests would still pass. **Fix:** Add assertions to `atomic_group_all_members_merge_in_one_cycle` (or a dedicated test) that verify: (1) each author receives a merge notification after successful group merge, and (2) analytics events are recorded for each merged member with `"atomic_group"` in the payload.

## Verification Summary

- F1 structural fix: correct — side effects deferred to post-loop success block
- F2 structural fix: correct — audience restricted to authors only
- F2 test: correct — negative assertion verifies workspace member exclusion
- All 31 merge_processor tests pass
- Mechanical checks pass: `check-transactional-side-effects.sh`, `check-notification-audience.sh`, `check-event-emission-coverage.sh`, `check-assertionless-tests.sh`
- R2 gap: the new deferred code path (lines 620-648) is untested — a regression that removes the block would be undetected
