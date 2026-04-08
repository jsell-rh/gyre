//! Notification types shared across all crates (HSI §2).
//!
//! The `Notification` struct is a wire type used by the adapters, server, and clients.
//! It lives here in `gyre-common` (alongside `Message` and `Id`) so that all crates
//! can reference it without pulling in domain logic.

use crate::Id;
use serde::{Deserialize, Serialize};

/// The set of inbox item types (HSI §2 / §8 priority table).
///
/// Variants are stored as their exact string name in the database.
/// Default priority for each variant is shown in comments; callers SHOULD use
/// `NotificationType::default_priority()` when creating notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationType {
    /// Priority 1 — agent needs human clarification before continuing.
    AgentNeedsClarification,
    /// Priority 2 — spec saved via human editor is awaiting approval gate.
    SpecPendingApproval,
    /// Priority 3 — a quality gate failed on an MR.
    GateFailure,
    /// Priority 4 — a spec linked cross-workspace has changed.
    CrossWorkspaceSpecChange,
    /// Priority 5 — two agents have produced conflicting interpretations.
    ConflictingInterpretations,
    /// Priority 6 — workspace meta-spec has drifted from its canonical source.
    MetaSpecDrift,
    /// Priority 7 — agent budget is running low.
    BudgetWarning,
    /// Priority 8 — trust system suggests a new trust policy.
    TrustSuggestion,
    /// Priority 9 — a spec assertion failed during gate evaluation.
    SpecAssertionFailure,
    /// Priority 9 — an agent branch has been abandoned.
    AbandonedBranch,
    /// Priority 9 — an agent completed its task and created an MR for review.
    AgentCompleted,
    /// Priority 5 — an agent has escalated or failed and needs human attention.
    AgentEscalation,
    /// Priority 10 — knowledge-graph extraction suggests a spec link.
    SuggestedSpecLink,
    /// Priority 2 — a spec was rejected while agents were implementing it.
    SpecRejected,
    /// Priority 3 — a constraint violation was detected at push or merge time (§7.5).
    ConstraintViolation,
}

impl NotificationType {
    /// Returns the canonical string used for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentNeedsClarification => "AgentNeedsClarification",
            Self::SpecPendingApproval => "SpecPendingApproval",
            Self::GateFailure => "GateFailure",
            Self::CrossWorkspaceSpecChange => "CrossWorkspaceSpecChange",
            Self::ConflictingInterpretations => "ConflictingInterpretations",
            Self::MetaSpecDrift => "MetaSpecDrift",
            Self::BudgetWarning => "BudgetWarning",
            Self::TrustSuggestion => "TrustSuggestion",
            Self::SpecAssertionFailure => "SpecAssertionFailure",
            Self::AbandonedBranch => "AbandonedBranch",
            Self::AgentCompleted => "AgentCompleted",
            Self::AgentEscalation => "AgentEscalation",
            Self::SuggestedSpecLink => "SuggestedSpecLink",
            Self::SpecRejected => "SpecRejected",
            Self::ConstraintViolation => "ConstraintViolation",
        }
    }

    /// Parses from the DB string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "AgentNeedsClarification" => Some(Self::AgentNeedsClarification),
            "SpecPendingApproval" => Some(Self::SpecPendingApproval),
            "GateFailure" => Some(Self::GateFailure),
            "CrossWorkspaceSpecChange" => Some(Self::CrossWorkspaceSpecChange),
            "ConflictingInterpretations" => Some(Self::ConflictingInterpretations),
            "MetaSpecDrift" => Some(Self::MetaSpecDrift),
            "BudgetWarning" => Some(Self::BudgetWarning),
            "TrustSuggestion" => Some(Self::TrustSuggestion),
            "SpecAssertionFailure" => Some(Self::SpecAssertionFailure),
            "AbandonedBranch" => Some(Self::AbandonedBranch),
            "AgentCompleted" => Some(Self::AgentCompleted),
            "AgentEscalation" => Some(Self::AgentEscalation),
            "SuggestedSpecLink" => Some(Self::SuggestedSpecLink),
            "SpecRejected" => Some(Self::SpecRejected),
            "ConstraintViolation" => Some(Self::ConstraintViolation),
            _ => None,
        }
    }

    /// The default priority (1–10) for this notification type per HSI §8.
    pub fn default_priority(&self) -> u8 {
        match self {
            Self::AgentNeedsClarification => 1,
            Self::SpecPendingApproval => 2,
            Self::GateFailure => 3,
            Self::CrossWorkspaceSpecChange => 4,
            Self::ConflictingInterpretations => 5,
            Self::MetaSpecDrift => 6,
            Self::BudgetWarning => 7,
            Self::TrustSuggestion => 8,
            Self::SpecAssertionFailure => 9,
            Self::AbandonedBranch => 9,
            Self::AgentCompleted => 9,
            Self::AgentEscalation => 5,
            Self::SuggestedSpecLink => 10,
            Self::SpecRejected => 2,
            Self::ConstraintViolation => 3,
        }
    }
}

/// An Inbox item delivered to a specific user (HSI §2).
///
/// Stored in the `notifications` table. The Inbox badge count is the count of
/// notifications where `resolved_at IS NULL AND dismissed_at IS NULL`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Id,
    pub workspace_id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    /// Priority 1 (highest urgency) to 10 (lowest). See HSI §8.
    pub priority: u8,
    pub title: String,
    /// Optional JSON payload with type-specific data.
    pub body: Option<String>,
    /// Optional reference to the entity (spec_path, agent_id, mr_id, etc.).
    pub entity_ref: Option<String>,
    /// Optional repository id for repo-scope Inbox filtering.
    pub repo_id: Option<String>,
    /// Set when the human takes action (approves, resolves, etc.). Epoch seconds.
    pub resolved_at: Option<i64>,
    /// Set when the human explicitly dismisses without acting. Epoch seconds.
    /// Used by trust suggestions to suppress re-creation for 30 days.
    pub dismissed_at: Option<i64>,
    pub created_at: i64,
    pub tenant_id: String,
}

impl Notification {
    pub fn new(
        id: Id,
        workspace_id: Id,
        user_id: Id,
        notification_type: NotificationType,
        title: impl Into<String>,
        tenant_id: impl Into<String>,
        now: i64,
    ) -> Self {
        let priority = notification_type.default_priority();
        Self {
            id,
            workspace_id,
            user_id,
            notification_type,
            priority,
            title: title.into(),
            body: None,
            entity_ref: None,
            repo_id: None,
            resolved_at: None,
            dismissed_at: None,
            created_at: now,
            tenant_id: tenant_id.into(),
        }
    }

    /// Returns true if this notification is active (not resolved or dismissed).
    pub fn is_active(&self) -> bool {
        self.resolved_at.is_none() && self.dismissed_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_priorities_match_spec() {
        assert_eq!(
            NotificationType::AgentNeedsClarification.default_priority(),
            1
        );
        assert_eq!(NotificationType::SpecPendingApproval.default_priority(), 2);
        assert_eq!(NotificationType::GateFailure.default_priority(), 3);
        assert_eq!(
            NotificationType::CrossWorkspaceSpecChange.default_priority(),
            4
        );
        assert_eq!(NotificationType::SuggestedSpecLink.default_priority(), 10);
    }

    #[test]
    fn round_trip_type_strings() {
        let variants = [
            NotificationType::AgentNeedsClarification,
            NotificationType::SpecPendingApproval,
            NotificationType::GateFailure,
            NotificationType::CrossWorkspaceSpecChange,
            NotificationType::ConflictingInterpretations,
            NotificationType::MetaSpecDrift,
            NotificationType::BudgetWarning,
            NotificationType::TrustSuggestion,
            NotificationType::SpecAssertionFailure,
            NotificationType::AbandonedBranch,
            NotificationType::AgentCompleted,
            NotificationType::AgentEscalation,
            NotificationType::SuggestedSpecLink,
            NotificationType::SpecRejected,
            NotificationType::ConstraintViolation,
        ];
        for v in &variants {
            assert_eq!(NotificationType::parse(v.as_str()).as_ref(), Some(v));
        }
    }

    #[test]
    fn new_notification_is_active() {
        let n = Notification::new(
            Id::new("n1"),
            Id::new("ws1"),
            Id::new("u1"),
            NotificationType::GateFailure,
            "Gate failed",
            "tenant1",
            1000,
        );
        assert!(n.is_active());
        assert_eq!(n.priority, 3);
    }
}
