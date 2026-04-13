---
title: "Abandoned Branch Detection Job"
spec_ref: "human-system-interface.md §8 (Inbox Detail, priority 9: Abandoned Branch)"
depends_on: []
progress: complete
review: specs/reviews/task-056.md
coverage_sections: []
commits: []
---

## Spec Excerpt

From `human-system-interface.md` §8:

> | 9 | **Spec assertion failure** | Knowledge graph + assertions | Fix code or update spec |

Alongside `SpecAssertionFailure`, the `AbandonedBranch` notification type (also priority 9) is defined in the `NotificationType` enum:

```rust
AbandonedBranch,  // priority 9
```

From `human-system-interface.md` §1 (Deep Links):

> A background job (`abandoned_branch_check`) runs daily. It flags `spec-edit/*` branches with no commits for 7 days. Creates Notification items (priority 9). Uses MR `updated_at` field.

And from `ui-layout.md` §3:

> Abandoned branches flagged by daily job.

## Current State

- **Job registration:** The `abandoned_branch_check` job is registered in `crates/gyre-server/src/jobs.rs:294–313` with correct metadata (daily interval, description). However the closure body is a **stub** — it logs `"abandoned_branch_check: stub, no-op"` and returns `Ok(())`.
- **Notification type:** `NotificationType::AbandonedBranch` exists in `crates/gyre-common/src/notification.rs:36` with priority 9.
- **MR port:** `MergeRequestRepository::list_by_workspace` exists in `crates/gyre-ports/src/merge_request.rs:17`.
- **Workspace membership port:** `WorkspaceMembershipRepository::list_by_workspace` exists (used by `trust_suggestion.rs` already).
- **Notification port:** `NotificationRepository::create`, `has_recent_dismissal` — all exist and are tested (see `trust_suggestion.rs` for the pattern).

Missing:
- The actual job logic (query MRs → filter → create notifications)

## Implementation Plan

1. **Create `crates/gyre-server/src/abandoned_branch.rs`** — follow the same pattern as `trust_suggestion.rs`:
   - `pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()>` — iterate all workspaces.
   - For each workspace, call `state.merge_requests.list_by_workspace(&ws.id)`.
   - Filter to MRs where:
     - `source_branch` starts with `"spec-edit/"`
     - `status` is `Open` (not merged, not closed)
     - `updated_at` < now − 604800 (7 days in seconds)
   - For each matching MR, create priority-9 `AbandonedBranch` notifications for workspace `Admin` and `Developer` members.
   - **Suppression:** Before creating, check `has_recent_dismissal` (30 days, same pattern as trust suggestion) to avoid re-notifying users who already dismissed the notification for this MR.
   - **Deduplication:** Check for existing active `AbandonedBranch` notifications with the same `entity_ref` (MR id) before creating new ones.

2. **Wire into jobs.rs:**
   - Replace the stub closure at `jobs.rs:305–312` with:
     ```rust
     |state| async move { crate::abandoned_branch::run_once(&state).await },
     ```
   - Add `pub mod abandoned_branch;` to `lib.rs`.

3. **Notification content:**
   - Title: `"Abandoned branch: '{source_branch}' — MR '{title}' has had no activity for >7 days"`
   - `entity_ref`: the MR id (for linking from Inbox to MR detail)
   - `repo_id`: the MR's `repository_id` (for repo-scope Inbox filtering)
   - `workspace_id`: from the MR's workspace
   - `tenant_id`: from the workspace

4. **Tests** (in `abandoned_branch.rs`):
   - Test: creates notification when MR has `spec-edit/` branch and `updated_at` > 7 days ago
   - Test: does NOT create notification when MR branch doesn't start with `spec-edit/`
   - Test: does NOT create notification when MR is merged or closed
   - Test: does NOT create notification when `updated_at` is within 7 days
   - Test: suppresses notification when user recently dismissed one
   - Test: does NOT duplicate notification when one already exists for the MR

## Acceptance Criteria

- [ ] `abandoned_branch_check` job is no longer a stub
- [ ] Creates `AbandonedBranch` notifications (priority 9) for `spec-edit/*` MRs with no activity for >7 days
- [ ] Notifications sent to workspace Admin and Developer members
- [ ] Notifications include `entity_ref` (MR id) and `repo_id` for Inbox filtering
- [ ] Dismissed notifications are not re-created within 30 days
- [ ] Active notifications for the same MR are not duplicated
- [ ] Merged/closed MRs are not flagged
- [ ] Unit tests cover all cases
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/trust_suggestion.rs` — use it as the template (same job pattern)
3. Read `crates/gyre-server/src/jobs.rs:294–313` — find the stub to replace
4. Read `crates/gyre-common/src/notification.rs` — find `AbandonedBranch` variant and `Notification::new`
5. Read `crates/gyre-ports/src/merge_request.rs` — find `list_by_workspace` for the MR query
6. Create `crates/gyre-server/src/abandoned_branch.rs` following the trust_suggestion pattern
7. Replace the stub in jobs.rs with the real closure
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(server): implement abandoned branch detection job (TASK-056)`
