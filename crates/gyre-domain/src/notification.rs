use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationType {
    // Approvals
    SpecApprovalRequested,
    PersonaApprovalRequested,
    // Agent escalations
    AgentEscalation,
    AgentBudgetWarning,
    AgentBudgetExhausted,
    AgentFailed,
    // Gate results
    GateFailure,
    GatePassed,
    // Merge queue
    MrMerged,
    MrNeedsReview,
    MrReverted,
    // Dependencies
    BreakingChangeDetected,
    SpecDriftDetected,
    // Workspace
    InvitationReceived,
    MembershipChanged,
    // System
    SystemAlert,
    // Cross-workspace spec dependencies
    CrossWorkspaceSpecChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NotificationPriority {
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub priority: NotificationPriority,
    pub action_url: Option<String>,
    pub read: bool,
    pub read_at: Option<u64>,
    pub created_at: u64,
}

impl Notification {
    pub fn new(
        id: Id,
        user_id: Id,
        notification_type: NotificationType,
        title: impl Into<String>,
        body: impl Into<String>,
        priority: NotificationPriority,
        now: u64,
    ) -> Self {
        Self {
            id,
            user_id,
            notification_type,
            title: title.into(),
            body: body.into(),
            entity_type: None,
            entity_id: None,
            priority,
            action_url: None,
            read: false,
            read_at: None,
            created_at: now,
        }
    }

    pub fn mark_read(&mut self, now: u64) {
        self.read = true;
        self.read_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_starts_unread() {
        let n = Notification::new(
            Id::new("n1"),
            Id::new("u1"),
            NotificationType::GateFailure,
            "Gate failed",
            "The lint gate failed on MR #42",
            NotificationPriority::High,
            1000,
        );
        assert!(!n.read);
        assert!(n.read_at.is_none());
    }

    #[test]
    fn mark_read() {
        let mut n = Notification::new(
            Id::new("n1"),
            Id::new("u1"),
            NotificationType::MrMerged,
            "MR merged",
            "Your MR was merged",
            NotificationPriority::Low,
            1000,
        );
        n.mark_read(2000);
        assert!(n.read);
        assert_eq!(n.read_at, Some(2000));
    }

    #[test]
    fn priority_ordering() {
        assert!(NotificationPriority::Urgent > NotificationPriority::High);
        assert!(NotificationPriority::High > NotificationPriority::Medium);
        assert!(NotificationPriority::Medium > NotificationPriority::Low);
    }
}
