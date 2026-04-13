---
title: "HSI Trust Suggestions Background Job"
spec_ref: "human-system-interface.md §2 Trust Suggestions"
depends_on:
  - task-077
progress: not-started
coverage_sections:
  - "human-system-interface.md §2 Trust Suggestions"
commits: []
---

## Spec Excerpt

The system can suggest increasing trust based on track record:

```
This workspace has had 0 gate failures and 0 reverted MRs in 30 days.
Consider increasing trust level to Autonomous.
[Increase Trust] [Dismiss]
```

This appears as an Inbox item (priority 8). The human decides.

**Mechanism:** A background job (`trust_suggestion_check`) runs daily per workspace. It queries gate results and MR reverts for the last 30 days. If both counts are 0 and the current trust level is not already Autonomous, and the user has not dismissed this suggestion in the last 30 days, it creates one `Notification` per workspace Admin member. The job is registered in the server's `JobRegistry`.

## Implementation Plan

1. **Register `trust_suggestion_check` job** in the server's `JobRegistry`:
   - Runs daily (24h interval)
   - Iterates over all workspaces

2. **Per-workspace check logic:**
   - Query gate results for last 30 days: count failures (gate status != pass)
   - Query MRs with status `Reverted` for last 30 days
   - Skip if `workspace.trust_level == Autonomous`
   - Skip if any Admin member has dismissed a TrustSuggestion notification in last 30 days (use `NotificationRepository::has_recent_dismissal`)

3. **Notification creation:**
   - If both failure count and revert count are 0:
     - Create one `Notification` per workspace Admin/Developer member:
       - `notification_type: TrustSuggestion`
       - `priority: 8`
       - `title: "Consider increasing trust level"`
       - `body: { "gate_failures": 0, "mr_reverts": 0, "period_days": 30, "current_level": "Guided", "suggested_level": "Autonomous" }`
       - `workspace_id: workspace.id`
       - `repo_id: None` (workspace-level suggestion)
   - Each admin gets their own notification and can dismiss independently

4. **Dismissal tracking:**
   - When an admin dismisses the notification, `dismissed_at` is set
   - The job checks `has_recent_dismissal` per user before creating new notifications
   - 30-day cooldown per user per workspace

5. **Inline actions:**
   - "Increase Trust" → client calls `PUT /api/v1/workspaces/:id` with `trust_level: "Autonomous"` and resolves the notification
   - "Dismiss" → client calls `POST /api/v1/notifications/:id/dismiss`

## Acceptance Criteria

- [ ] `trust_suggestion_check` job registered in JobRegistry
- [ ] Job runs daily per workspace
- [ ] Skips workspaces already at Autonomous trust level
- [ ] Queries gate failures and MR reverts for last 30 days
- [ ] Creates notifications only when both counts are 0
- [ ] Creates one notification per Admin member (not one per workspace)
- [ ] Respects 30-day dismissal cooldown per user
- [ ] Notification has correct type (TrustSuggestion), priority (8), and body fields
- [ ] Unit test for job logic: workspace with clean 30-day record generates suggestion
- [ ] Unit test for job logic: workspace with failures does NOT generate suggestion
- [ ] Unit test for dismissal cooldown: dismissed < 30 days ago → no new notification
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/human-system-interface.md` §2 "Trust Suggestions" for the full requirement. The `JobRegistry` exists in the server — check how existing background jobs are registered (look for `admin_run_job` or similar patterns in `crates/gyre-server/src/`). The `NotificationRepository` port already exists in `crates/gyre-ports/src/notification.rs` — use its `has_recent_dismissal` method for the 30-day cooldown check. The `Notification` struct is in `crates/gyre-common/src/notification.rs`. You need to determine the workspace's trust level from the workspace entity. Gate results should be queryable from the gate evaluation records. MR reverts require checking MR status for `Reverted` — verify this status variant exists in the MR model (the spec says to add `Reverted` to the MR status enum if not present).
