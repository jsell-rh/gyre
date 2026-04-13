---
title: "Implement notification entity with full type taxonomy and priority model"
spec_ref: "user-management.md §Notification Entity"
depends_on:
  - task-120
progress: not-started
coverage_sections:
  - "user-management.md §Notification Entity"
commits: []
---

## Spec Excerpt

From `user-management.md` §Notification Entity:

```rust
pub struct Notification {
    pub id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    pub source: NotificationSource,
    pub priority: NotificationPriority,
    pub action_url: Option<String>,  // Deep link to relevant page
    pub read: bool,
    pub read_at: Option<u64>,
    pub created_at: u64,
}

pub enum NotificationType {
    SpecApprovalRequested,
    PersonaApprovalRequested,
    AgentEscalation,
    AgentBudgetWarning,
    AgentBudgetExhausted,
    AgentFailed,
    GateFailure,
    GatePassed,
    MrMerged,
    MrReverted,
    MergeQueuePaused,
    BreakingChangeDetected,
    SpecDriftDetected,
    DependencyStale,
    SecurityFinding,
    InvitationReceived,
    MembershipChanged,
    SystemAlert,
}

pub enum NotificationPriority {
    Low,       // Informational (MR merged, gate passed)
    Medium,    // Needs attention eventually (stale dependency, spec drift)
    High,      // Needs attention soon (budget warning, gate failure)
    Urgent,    // Needs attention now (security finding, agent escalation, MR reverted)
}

pub struct NotificationSource {
    pub entity_type: String,  // "agent", "mr", "spec", "gate", "budget"
    pub entity_id: String,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,
}
```

## Implementation Plan

1. **Audit existing notification model:**
   - Check existing `NotificationType` in `gyre-common/src/notification.rs`
   - Check existing notification storage and API
   - Compare existing types against spec's 18-variant taxonomy

2. **Enhance NotificationType:**
   - Add missing variants from spec taxonomy
   - Existing 10 variants (per HSI coverage) → spec requires 18
   - Missing variants likely include: PersonaApprovalRequested, AgentBudgetExhausted, AgentFailed, GatePassed, MrReverted, MergeQueuePaused, BreakingChangeDetected, DependencyStale

3. **Add NotificationPriority:**
   - Low, Medium, High, Urgent enum
   - Map each NotificationType to a default priority per spec

4. **Add NotificationSource:**
   - Structured source tracking: entity_type, entity_id, workspace_id, repo_id
   - Replaces any free-form source fields

5. **Enhance Notification entity:**
   - Ensure all spec fields present: title, body, source, priority, action_url, read, read_at
   - Migration if needed for new columns

6. **Update notification creation paths:**
   - Audit all places notifications are created
   - Ensure they use the correct NotificationType, priority, and source

## Acceptance Criteria

- [ ] Notification entity has all spec'd fields (id, user_id, notification_type, title, body, source, priority, action_url, read, read_at, created_at)
- [ ] All 18 NotificationType variants defined
- [ ] NotificationPriority enum: Low, Medium, High, Urgent
- [ ] NotificationSource struct with entity_type, entity_id, workspace_id, repo_id
- [ ] Default priority mapping per notification type
- [ ] Action URL populated for deep-link navigation
- [ ] Existing notification creation paths updated to use new types
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Notification Entity. Existing notification types: `gyre-common/src/notification.rs`. Notification storage: grep for `NotificationRepository` in `gyre-ports/` and `gyre-adapters/`. Notification creation: grep for `create_notification\|Notification::new\|NotificationType::` in `gyre-server/`. Notification API: `gyre-server/src/api/users.rs` (get_my_notifications, get_notification_count).
