//! Notification types shared across all crates (HSI §2, user-management.md
//! §Notification Entity).
//!
//! The `Notification` struct is a wire type used by the adapters, server, and clients.
//! It lives here in `gyre-common` (alongside `Message` and `Id`) so that all crates
//! can reference it without pulling in domain logic.
//!
//! Two priority models coexist by design:
//!
//! - `NotificationPriority` (user-management.md §Notification Entity) is the
//!   entity's four-level priority (Low / Medium / High / Urgent).
//! - `priority_rank` (HSI §8) is the 1–10 numeric rank used for Inbox sorting
//!   and the `min_priority`/`max_priority` list filters.
//!
//! The two are deliberately **not** mapped onto each other (amended
//! §Notification Entity membership table, task-112 review): the band derives
//! from the notification TYPE via `NotificationPriority::for_notification_type`,
//! because no numeric threshold can reproduce the specced membership
//! (SecurityFinding and GateFailure share numeric priority 3 yet sit in
//! different bands; AgentEscalation is numeric 5 yet Urgent; BudgetWarning is
//! numeric 7 yet High). Deriving the band from the rank — at creation or at
//! read time — is a spec violation, not a simplification.

use crate::Id;
use serde::{Deserialize, Serialize};

/// The notification type taxonomy (user-management.md §Notification Entity,
/// plus HSI §2 inbox types that have no user-management counterpart).
///
/// Variants are stored as their exact string name in the database.
/// `NotificationType::parse` also accepts the pre-user-management names of
/// renamed variants (`SpecPendingApproval`, `BudgetWarning`) so rows written
/// before the taxonomy unification keep parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationType {
    // ── user-management.md §Notification Entity taxonomy ─────────────────────
    // Approvals
    /// A spec was saved and is awaiting its approval gate.
    SpecApprovalRequested,
    /// A persona was created and is awaiting approval (routes to its owner).
    PersonaApprovalRequested,

    // Agent escalations
    /// An agent escalated and needs a human now.
    AgentEscalation,
    /// An agent's budget passed the warning threshold (80%).
    AgentBudgetWarning,
    /// An agent's budget is exhausted; work is stopped.
    AgentBudgetExhausted,
    /// An agent failed and needs attention.
    AgentFailed,

    // Gate results
    /// A quality gate failed on an MR.
    GateFailure,
    /// All quality gates passed on an MR.
    GatePassed,

    // Merge queue
    /// An MR was merged.
    MrMerged,
    /// A merged MR was reverted by the post-merge recovery protocol.
    MrReverted,
    /// The merge queue for a repo is paused (broken main).
    MergeQueuePaused,

    // Dependencies
    /// A breaking change was detected in a repo others depend on.
    BreakingChangeDetected,
    /// Spec drift was detected by the spec patrol.
    SpecDriftDetected,
    /// A dependency edge went stale (version drift / unverified too long).
    DependencyStale,

    // Security
    /// A security finding (Critical/High) needs attention now.
    SecurityFinding,

    // Workspace
    /// A user was invited to a workspace.
    InvitationReceived,
    /// A user's workspace membership changed.
    MembershipChanged,

    // System
    /// Platform-level system alert.
    SystemAlert,

    // ── HSI §2 inbox types without a user-management counterpart ────────────
    /// Priority 1 — agent needs human clarification before continuing.
    AgentNeedsClarification,
    /// Priority 4 — a spec linked cross-workspace has changed.
    CrossWorkspaceSpecChange,
    /// Priority 5 — two agents have produced conflicting interpretations.
    ConflictingInterpretations,
    /// Priority 6 — workspace meta-spec has drifted from its canonical source.
    MetaSpecDrift,
    /// Priority 8 — trust system suggests a new trust policy.
    TrustSuggestion,
    /// Priority 9 — a spec assertion failed during gate evaluation.
    SpecAssertionFailure,
    /// Priority 9 — an agent branch has been abandoned.
    AbandonedBranch,
    /// Priority 9 — an agent completed its task and created an MR for review.
    AgentCompleted,
    /// Priority 10 — knowledge-graph extraction suggests a spec link.
    SuggestedSpecLink,
    /// Priority 2 — a spec was rejected while agents were implementing it.
    SpecRejected,
    /// Priority 2 — a constraint violation was detected at push or merge time (§7.5).
    ConstraintViolation,
    /// Priority 4 — cascade test triggered for a dependent repo after a merge.
    CascadeTestTriggered,
    /// Priority 3 — cascade test failed in a dependent repo.
    CascadeTestFailed,
    /// Priority 7 — MR dependency chain exceeds 10 levels (decomposition smell).
    DependencyChainTooDeep,
    /// Priority 3 — an atomic group member failed to merge; entire group rolled back.
    AtomicGroupFailure,
    /// Priority 2 — two humans edited the same spec concurrently and the second
    /// save was rejected (HSI §7 Conflict Prevention). Created for both editors.
    SpecConflict,
    /// Priority 1 — post-merge revert re-validation failed on the reverted
    /// HEAD; queue stays paused until a human intervenes (platform-model.md §6).
    MergeQueueEscalation,
    /// Priority 1 — a worker spawn failed on the workspace's compute target
    /// after all retries; the task is Blocked and needs operator attention
    /// (agent-runtime.md §1 Phase 7 "Spawn failure").
    ComputeTargetUnavailable,
    /// Priority 2 — an admin requested to impersonate this user; the body
    /// carries the approval code the user must submit to consent
    /// (identity-security.md §Impersonation).
    ImpersonationRequest,
    /// Priority 1 — an admin began impersonating this user
    /// (identity-security.md §Impersonation).
    ImpersonationAlert,
    /// Priority 7 — a rotated secret approaches expiry (platform-model.md §7).
    SecretExpiring,
    /// Priority 2 — a repo-defined persona file changed on push and its
    /// approval was reset to Pending (platform-model.md §2 Repo-Defined
    /// Personas). Sent to the persona owner.
    PersonaChangePendingApproval,
    /// Priority 2 — a workspace has no active Owner and the tenant admin
    /// must reassign ownership (user-management.md §Ownership Transfer &
    /// Reclamation). Sent to every TenantAdmin of the workspace's tenant.
    WorkspaceOwnerless,
}

impl NotificationType {
    /// Returns the canonical string used for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            // user-management.md §Notification Entity
            Self::SpecApprovalRequested => "SpecApprovalRequested",
            Self::PersonaApprovalRequested => "PersonaApprovalRequested",
            Self::AgentEscalation => "AgentEscalation",
            Self::AgentBudgetWarning => "AgentBudgetWarning",
            Self::AgentBudgetExhausted => "AgentBudgetExhausted",
            Self::AgentFailed => "AgentFailed",
            Self::GateFailure => "GateFailure",
            Self::GatePassed => "GatePassed",
            Self::MrMerged => "MrMerged",
            Self::MrReverted => "MrReverted",
            Self::MergeQueuePaused => "MergeQueuePaused",
            Self::BreakingChangeDetected => "BreakingChangeDetected",
            Self::SpecDriftDetected => "SpecDriftDetected",
            Self::DependencyStale => "DependencyStale",
            Self::SecurityFinding => "SecurityFinding",
            Self::InvitationReceived => "InvitationReceived",
            Self::MembershipChanged => "MembershipChanged",
            Self::SystemAlert => "SystemAlert",
            // HSI §2
            Self::AgentNeedsClarification => "AgentNeedsClarification",
            Self::CrossWorkspaceSpecChange => "CrossWorkspaceSpecChange",
            Self::ConflictingInterpretations => "ConflictingInterpretations",
            Self::MetaSpecDrift => "MetaSpecDrift",
            Self::TrustSuggestion => "TrustSuggestion",
            Self::SpecAssertionFailure => "SpecAssertionFailure",
            Self::AbandonedBranch => "AbandonedBranch",
            Self::AgentCompleted => "AgentCompleted",
            Self::SuggestedSpecLink => "SuggestedSpecLink",
            Self::SpecRejected => "SpecRejected",
            Self::ConstraintViolation => "ConstraintViolation",
            Self::CascadeTestTriggered => "CascadeTestTriggered",
            Self::CascadeTestFailed => "CascadeTestFailed",
            Self::DependencyChainTooDeep => "DependencyChainTooDeep",
            Self::AtomicGroupFailure => "AtomicGroupFailure",
            Self::SpecConflict => "SpecConflict",
            Self::MergeQueueEscalation => "MergeQueueEscalation",
            Self::ComputeTargetUnavailable => "ComputeTargetUnavailable",
            Self::ImpersonationRequest => "ImpersonationRequest",
            Self::ImpersonationAlert => "ImpersonationAlert",
            Self::SecretExpiring => "SecretExpiring",
            Self::PersonaChangePendingApproval => "PersonaChangePendingApproval",
            Self::WorkspaceOwnerless => "WorkspaceOwnerless",
        }
    }

    /// Parses from the DB string representation.
    ///
    /// Accepts the pre-unification names of renamed variants so rows written
    /// before user-management.md §Notification Entity landed keep parsing:
    /// `SpecPendingApproval` → `SpecApprovalRequested`,
    /// `BudgetWarning` → `AgentBudgetWarning`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            // user-management.md §Notification Entity
            "SpecApprovalRequested" | "SpecPendingApproval" => Some(Self::SpecApprovalRequested),
            "PersonaApprovalRequested" => Some(Self::PersonaApprovalRequested),
            "AgentEscalation" => Some(Self::AgentEscalation),
            "AgentBudgetWarning" | "BudgetWarning" => Some(Self::AgentBudgetWarning),
            "AgentBudgetExhausted" | "BudgetExhausted" => Some(Self::AgentBudgetExhausted),
            "AgentFailed" => Some(Self::AgentFailed),
            "GateFailure" => Some(Self::GateFailure),
            "GatePassed" => Some(Self::GatePassed),
            "MrMerged" => Some(Self::MrMerged),
            "MrReverted" => Some(Self::MrReverted),
            "MergeQueuePaused" => Some(Self::MergeQueuePaused),
            "BreakingChangeDetected" => Some(Self::BreakingChangeDetected),
            "SpecDriftDetected" => Some(Self::SpecDriftDetected),
            "DependencyStale" => Some(Self::DependencyStale),
            "SecurityFinding" => Some(Self::SecurityFinding),
            "InvitationReceived" => Some(Self::InvitationReceived),
            "MembershipChanged" => Some(Self::MembershipChanged),
            "SystemAlert" => Some(Self::SystemAlert),
            // HSI §2
            "AgentNeedsClarification" => Some(Self::AgentNeedsClarification),
            "CrossWorkspaceSpecChange" => Some(Self::CrossWorkspaceSpecChange),
            "ConflictingInterpretations" => Some(Self::ConflictingInterpretations),
            "MetaSpecDrift" => Some(Self::MetaSpecDrift),
            "TrustSuggestion" => Some(Self::TrustSuggestion),
            "SpecAssertionFailure" => Some(Self::SpecAssertionFailure),
            "AbandonedBranch" => Some(Self::AbandonedBranch),
            "AgentCompleted" => Some(Self::AgentCompleted),
            "SuggestedSpecLink" => Some(Self::SuggestedSpecLink),
            "SpecRejected" => Some(Self::SpecRejected),
            "ConstraintViolation" => Some(Self::ConstraintViolation),
            "CascadeTestTriggered" => Some(Self::CascadeTestTriggered),
            "CascadeTestFailed" => Some(Self::CascadeTestFailed),
            "DependencyChainTooDeep" => Some(Self::DependencyChainTooDeep),
            "AtomicGroupFailure" => Some(Self::AtomicGroupFailure),
            "SpecConflict" => Some(Self::SpecConflict),
            "MergeQueueEscalation" => Some(Self::MergeQueueEscalation),
            "ComputeTargetUnavailable" => Some(Self::ComputeTargetUnavailable),
            "ImpersonationRequest" => Some(Self::ImpersonationRequest),
            "ImpersonationAlert" => Some(Self::ImpersonationAlert),
            "SecretExpiring" => Some(Self::SecretExpiring),
            "PersonaChangePendingApproval" => Some(Self::PersonaChangePendingApproval),
            "WorkspaceOwnerless" => Some(Self::WorkspaceOwnerless),
            _ => None,
        }
    }

    /// The default four-level priority for this notification type
    /// (user-management.md §Notification Entity amended membership table).
    ///
    /// Derived from the TYPE, never from the numeric rank — see
    /// [`NotificationPriority::for_notification_type`].
    pub fn default_priority(&self) -> NotificationPriority {
        NotificationPriority::for_notification_type(self)
    }

    /// The default HSI §8 numeric rank (1 highest – 10 lowest) for this
    /// notification type.
    ///
    /// Ranks are the Inbox-ordering values of the HSI §8 scale — independent
    /// of the four-level band (see the module doc: the amended §Notification
    /// Entity membership table is deliberately not threshold-mappable onto
    /// the numeric scale, so a type's rank and its band may disagree, e.g.
    /// `SpecApprovalRequested` is rank 2 yet High).
    /// `AgentEscalation` (2) and `AgentBudgetWarning` (4) follow
    /// user-management.md's explicit level examples, superseding the looser
    /// HSI draft ranks (5 and 7).
    pub fn default_priority_rank(&self) -> u8 {
        match self {
            // user-management.md §Notification Entity
            Self::SpecApprovalRequested => 2,
            Self::PersonaApprovalRequested => 2,
            Self::AgentEscalation => 2,
            Self::AgentBudgetWarning => 4,
            Self::AgentBudgetExhausted => 1,
            Self::AgentFailed => 3,
            Self::GateFailure => 3,
            Self::GatePassed => 9,
            Self::MrMerged => 9,
            Self::MrReverted => 2,
            Self::MergeQueuePaused => 3,
            Self::BreakingChangeDetected => 4,
            Self::SpecDriftDetected => 6,
            Self::DependencyStale => 7,
            Self::SecurityFinding => 1,
            Self::InvitationReceived => 10,
            Self::MembershipChanged => 10,
            Self::SystemAlert => 3,
            // HSI §2
            Self::AgentNeedsClarification => 1,
            Self::CrossWorkspaceSpecChange => 4,
            Self::ConflictingInterpretations => 5,
            Self::MetaSpecDrift => 6,
            Self::TrustSuggestion => 8,
            Self::SpecAssertionFailure => 9,
            Self::AbandonedBranch => 9,
            Self::AgentCompleted => 9,
            Self::SuggestedSpecLink => 10,
            Self::SpecRejected => 2,
            Self::ConstraintViolation => 2,
            Self::CascadeTestTriggered => 4,
            Self::CascadeTestFailed => 3,
            Self::DependencyChainTooDeep => 7,
            Self::AtomicGroupFailure => 3,
            Self::SpecConflict => 2,
            Self::MergeQueueEscalation => 1,
            Self::ComputeTargetUnavailable => 1,
            Self::ImpersonationRequest => 2,
            Self::ImpersonationAlert => 1,
            Self::SecretExpiring => 7,
            Self::PersonaChangePendingApproval => 2,
            Self::WorkspaceOwnerless => 2,
        }
    }
}

/// The four-level notification priority (user-management.md §Notification Entity).
///
/// - `Low` — informational (MR merged, gate passed)
/// - `Medium` — needs attention eventually (stale dependency, spec drift)
/// - `High` — needs attention soon (budget warning, gate failure)
/// - `Urgent` — needs attention now (security finding, agent escalation, MR reverted)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationPriority {
    Low,
    Medium,
    High,
    Urgent,
}

impl NotificationPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }

    /// Parses from the string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Low" => Some(Self::Low),
            "Medium" => Some(Self::Medium),
            "High" => Some(Self::High),
            "Urgent" => Some(Self::Urgent),
            _ => None,
        }
    }

    /// Case-insensitive parse (query-parameter filter input —
    /// user-management.md §API > Notifications `?priority=`).
    pub fn from_level_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }

    /// The HSI §8 rank band this level covers (inclusive; rank 1 is
    /// highest): Urgent 1–2, High 3–4, Medium 5–7, Low 8–10. The inverse of
    /// `from_rank` — `from_rank(band.0) == self`.
    pub fn rank_band(&self) -> (u8, u8) {
        match self {
            Self::Urgent => (1, 2),
            Self::High => (3, 4),
            Self::Medium => (5, 7),
            Self::Low => (8, 10),
        }
    }

    /// Derives the four-level priority from an HSI §8 numeric rank.
    ///
    /// Bands: 1–2 Urgent, 3–4 High, 5–7 Medium, 8–10 Low. Ranks outside 1–10
    /// clamp to the nearest band.
    ///
    /// This describes the bands of the NUMERIC scale (used to widen a
    /// four-level `?priority=` list filter onto the stored rank column). It
    /// is NOT the derivation of the entity's `priority` field — that derives
    /// from the notification type via [`Self::for_notification_type`], per
    /// the amended §Notification Entity membership table.
    pub fn from_rank(rank: u8) -> Self {
        match rank.clamp(1, 10) {
            1..=2 => Self::Urgent,
            3..=4 => Self::High,
            5..=7 => Self::Medium,
            _ => Self::Low,
        }
    }

    /// The four-level band for a notification type, per the amended
    /// type→band membership table of user-management.md §Notification Entity
    /// (band comments + the task-112 review amendment recording the full
    /// table).
    ///
    /// This is the authoritative derivation of the `Notification` entity's
    /// `priority` field — used at creation time (`Notification::new`) and at
    /// read time (the SQL adapters reconstruct it from the persisted
    /// `notification_type`), and delegated to by the domain's channel-routing
    /// `for_notification_type` so the entity band and the routing band can
    /// never disagree. The band derives from the notification TYPE, never
    /// from the HSI §8 1–10 numeric priority: that scale is an
    /// Inbox-ordering concern and no numeric threshold can reproduce the
    /// specced membership (SecurityFinding and GateFailure share numeric
    /// priority 3 yet sit in different bands; AgentEscalation is numeric 5
    /// yet Urgent; BudgetWarning is numeric 7 yet High).
    pub fn for_notification_type(t: &NotificationType) -> Self {
        use NotificationType as T;
        match t {
            // Urgent — needs attention now (work blocked / immediate action):
            // security finding, agent escalation, MR reverted (spec examples),
            // agent stuck awaiting clarification (HSI §8 #1), merge queue
            // blocked, budget exhausted (work stopped), active impersonation
            // of the user, worker spawn failed after all retries.
            T::AgentEscalation
            | T::SecurityFinding
            | T::MrReverted
            | T::AgentNeedsClarification
            | T::MergeQueueEscalation
            | T::AgentBudgetExhausted
            | T::ImpersonationAlert
            | T::ComputeTargetUnavailable => Self::Urgent,
            // High — needs attention soon: gate failure, budget warning
            // (spec examples), plus decisions/blocks that stall work soon:
            // approvals, rejections, conflicts, paused queue, failed groups,
            // approaching deadlines (secret expiry), agent failure, breaking
            // changes, system alerts, ownerless workspaces.
            T::GateFailure
            | T::AgentBudgetWarning
            | T::SpecApprovalRequested
            | T::SpecRejected
            | T::ConstraintViolation
            | T::SpecConflict
            | T::PersonaApprovalRequested
            | T::PersonaChangePendingApproval
            | T::MergeQueuePaused
            | T::CascadeTestFailed
            | T::AtomicGroupFailure
            | T::ImpersonationRequest
            | T::SecretExpiring
            | T::AgentFailed
            | T::BreakingChangeDetected
            | T::SystemAlert
            | T::WorkspaceOwnerless => Self::High,
            // Medium — needs attention eventually: spec drift (spec example),
            // stale-dependency-adjacent and review-arbitration items.
            T::MetaSpecDrift
            | T::CrossWorkspaceSpecChange
            | T::ConflictingInterpretations
            | T::DependencyChainTooDeep
            | T::SpecAssertionFailure
            | T::SpecDriftDetected
            | T::DependencyStale => Self::Medium,
            // Low — informational (MR merged, gate passed — spec examples):
            // completions, suggestions, notices.
            T::AgentCompleted
            | T::TrustSuggestion
            | T::AbandonedBranch
            | T::SuggestedSpecLink
            | T::CascadeTestTriggered
            | T::MrMerged
            | T::GatePassed
            | T::InvitationReceived
            | T::MembershipChanged => Self::Low,
        }
    }
}

/// Structured source tracking for a notification
/// (user-management.md §Notification Entity).
///
/// `entity_type` is one of the source kinds the spec names — "agent", "mr",
/// "spec", "gate", "budget" — plus "workspace" and "system" for
/// workspace-level and platform-level events with no finer-grained entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSource {
    /// "agent", "mr", "spec", "gate", "budget", "workspace", "system"
    pub entity_type: String,
    pub entity_id: String,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,
}

impl NotificationSource {
    /// Source for notifications with no finer-grained entity: the workspace
    /// itself (trust suggestions, invitations, budget alerts, system alerts).
    pub fn workspace(workspace_id: Id) -> Self {
        let entity_id = workspace_id.as_str().to_string();
        Self {
            entity_type: "workspace".to_string(),
            entity_id,
            workspace_id,
            repo_id: None,
        }
    }
}

/// A notification delivered to a specific user
/// (user-management.md §Notification Entity, HSI §2).
///
/// Stored in the `notifications` table. The Inbox badge count is the count of
/// notifications where `resolved_at IS NULL AND dismissed_at IS NULL`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Id,
    pub workspace_id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    /// Four-level priority (user-management.md §Notification Entity).
    /// Derived from the notification type via the amended membership table
    /// (`NotificationPriority::for_notification_type`) — independent of
    /// `priority_rank`, which is the HSI §8 Inbox-ordering scale.
    pub priority: NotificationPriority,
    /// HSI §8 numeric rank (1 highest – 10 lowest) used for Inbox sorting and
    /// the min/max priority filters.
    pub priority_rank: u8,
    pub title: String,
    /// Optional JSON payload with type-specific data.
    pub body: Option<String>,
    /// Structured source: what entity this notification is about.
    pub source: NotificationSource,
    /// Deep link to the relevant page (user-management.md §Notification Entity).
    pub action_url: Option<String>,
    /// Whether the user has read the notification (in-app read state).
    pub read: bool,
    /// When the user read it. Epoch seconds.
    pub read_at: Option<i64>,
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
        let priority_rank = notification_type.default_priority_rank();
        Self {
            id,
            workspace_id: workspace_id.clone(),
            user_id,
            notification_type: notification_type.clone(),
            priority: notification_type.default_priority(),
            priority_rank,
            title: title.into(),
            body: None,
            source: NotificationSource::workspace(workspace_id),
            action_url: None,
            read: false,
            read_at: None,
            resolved_at: None,
            dismissed_at: None,
            created_at: now,
            tenant_id: tenant_id.into(),
        }
    }

    /// Returns true when this notification is active (not resolved or dismissed).
    pub fn is_active(&self) -> bool {
        self.resolved_at.is_none() && self.dismissed_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every user-management.md §Notification Entity variant must exist and
    /// round-trip through its canonical string.
    #[test]
    fn user_management_taxonomy_round_trips() {
        let variants = [
            NotificationType::SpecApprovalRequested,
            NotificationType::PersonaApprovalRequested,
            NotificationType::AgentEscalation,
            NotificationType::AgentBudgetWarning,
            NotificationType::AgentBudgetExhausted,
            NotificationType::AgentFailed,
            NotificationType::GateFailure,
            NotificationType::GatePassed,
            NotificationType::MrMerged,
            NotificationType::MrReverted,
            NotificationType::MergeQueuePaused,
            NotificationType::BreakingChangeDetected,
            NotificationType::SpecDriftDetected,
            NotificationType::DependencyStale,
            NotificationType::SecurityFinding,
            NotificationType::InvitationReceived,
            NotificationType::MembershipChanged,
            NotificationType::SystemAlert,
        ];
        assert_eq!(variants.len(), 18, "spec taxonomy has 18 variants");
        for v in &variants {
            assert_eq!(NotificationType::parse(v.as_str()).as_ref(), Some(v));
        }
    }

    /// Rows written before the taxonomy unification must keep parsing.
    #[test]
    fn legacy_variant_names_parse() {
        assert_eq!(
            NotificationType::parse("SpecPendingApproval"),
            Some(NotificationType::SpecApprovalRequested)
        );
        assert_eq!(
            NotificationType::parse("BudgetWarning"),
            Some(NotificationType::AgentBudgetWarning)
        );
    }

    /// The spec's band comments are authoritative level examples (Low =
    /// merged/passed, Medium = stale deps/spec drift, High = budget
    /// warning/gate failure, Urgent = security finding/agent escalation/MR
    /// reverted) — the default priority must match them. (The full table is
    /// pinned per variant by `entity_priority_matches_amended_membership_table`.)
    #[test]
    fn default_priority_bands_match_user_management_spec() {
        let cases = [
            (NotificationType::MrMerged, NotificationPriority::Low),
            (NotificationType::GatePassed, NotificationPriority::Low),
            (
                NotificationType::DependencyStale,
                NotificationPriority::Medium,
            ),
            (
                NotificationType::SpecDriftDetected,
                NotificationPriority::Medium,
            ),
            (
                NotificationType::AgentBudgetWarning,
                NotificationPriority::High,
            ),
            (NotificationType::GateFailure, NotificationPriority::High),
            (
                NotificationType::SecurityFinding,
                NotificationPriority::Urgent,
            ),
            (
                NotificationType::AgentEscalation,
                NotificationPriority::Urgent,
            ),
            (NotificationType::MrReverted, NotificationPriority::Urgent),
        ];
        for (t, level) in cases {
            assert_eq!(t.default_priority(), level, "{t:?}");
        }
    }

    /// The entity band must equal the amended type→band membership table
    /// (user-management.md §Notification Entity, task-112 review amendment)
    /// for EVERY variant — the table is spelled out here independently of
    /// `for_notification_type` so a drift in either direction fails this
    /// probe. The band must NOT be derivable from the numeric rank: several
    /// memberships are only expressible by type (e.g. SpecApprovalRequested
    /// is rank 2 yet High, CascadeTestTriggered is rank 4 yet Low,
    /// SecretExpiring is rank 7 yet High).
    #[test]
    fn entity_priority_matches_amended_membership_table() {
        use NotificationPriority as P;
        use NotificationType as T;
        let urgent = [
            T::SecurityFinding,
            T::AgentEscalation,
            T::MrReverted,
            T::AgentNeedsClarification,
            T::MergeQueueEscalation,
            T::AgentBudgetExhausted,
            T::ImpersonationAlert,
            T::ComputeTargetUnavailable,
        ];
        let high = [
            T::GateFailure,
            T::AgentBudgetWarning,
            T::SpecApprovalRequested,
            T::SpecRejected,
            T::ConstraintViolation,
            T::SpecConflict,
            T::PersonaApprovalRequested,
            T::PersonaChangePendingApproval,
            T::MergeQueuePaused,
            T::CascadeTestFailed,
            T::AtomicGroupFailure,
            T::SecretExpiring,
            T::ImpersonationRequest,
            T::AgentFailed,
            T::BreakingChangeDetected,
            T::SystemAlert,
            T::WorkspaceOwnerless,
        ];
        let medium = [
            T::MetaSpecDrift,
            T::CrossWorkspaceSpecChange,
            T::ConflictingInterpretations,
            T::DependencyChainTooDeep,
            T::SpecAssertionFailure,
            T::SpecDriftDetected,
            T::DependencyStale,
        ];
        let low = [
            T::AgentCompleted,
            T::TrustSuggestion,
            T::AbandonedBranch,
            T::SuggestedSpecLink,
            T::CascadeTestTriggered,
            T::MrMerged,
            T::GatePassed,
            T::InvitationReceived,
            T::MembershipChanged,
        ];

        // The four lists partition the taxonomy: every listed type appears
        // in exactly one band.
        let mut names: Vec<&str> = urgent
            .iter()
            .chain(&high)
            .chain(&medium)
            .chain(&low)
            .map(|t| t.as_str())
            .collect();
        let listed = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), listed, "a type is listed in two bands");

        for t in urgent {
            assert_eq!(t.default_priority(), P::Urgent, "{t:?} must be Urgent");
            assert_eq!(
                P::for_notification_type(&t),
                P::Urgent,
                "{t:?} must be Urgent"
            );
        }
        for t in high {
            assert_eq!(t.default_priority(), P::High, "{t:?} must be High");
            assert_eq!(P::for_notification_type(&t), P::High, "{t:?} must be High");
        }
        for t in medium {
            assert_eq!(t.default_priority(), P::Medium, "{t:?} must be Medium");
            assert_eq!(
                P::for_notification_type(&t),
                P::Medium,
                "{t:?} must be Medium"
            );
        }
        for t in low {
            assert_eq!(t.default_priority(), P::Low, "{t:?} must be Low");
            assert_eq!(P::for_notification_type(&t), P::Low, "{t:?} must be Low");
        }
    }

    #[test]
    fn priority_bands_from_rank() {
        assert_eq!(
            NotificationPriority::from_rank(1),
            NotificationPriority::Urgent
        );
        assert_eq!(
            NotificationPriority::from_rank(2),
            NotificationPriority::Urgent
        );
        assert_eq!(
            NotificationPriority::from_rank(3),
            NotificationPriority::High
        );
        assert_eq!(
            NotificationPriority::from_rank(4),
            NotificationPriority::High
        );
        assert_eq!(
            NotificationPriority::from_rank(5),
            NotificationPriority::Medium
        );
        assert_eq!(
            NotificationPriority::from_rank(7),
            NotificationPriority::Medium
        );
        assert_eq!(
            NotificationPriority::from_rank(8),
            NotificationPriority::Low
        );
        assert_eq!(
            NotificationPriority::from_rank(10),
            NotificationPriority::Low
        );
        // Out-of-range ranks clamp rather than panic.
        assert_eq!(
            NotificationPriority::from_rank(0),
            NotificationPriority::Urgent
        );
        assert_eq!(
            NotificationPriority::from_rank(255),
            NotificationPriority::Low
        );
    }

    /// rank_band is the inverse of from_rank (user-management.md §API >
    /// Notifications `?priority=` filter widens a level to its band), and
    /// from_level_str accepts the case-insensitive query form.
    #[test]
    fn rank_band_is_inverse_of_from_rank_and_level_str_parses() {
        for rank in 1u8..=10 {
            let level = NotificationPriority::from_rank(rank);
            let (lo, hi) = level.rank_band();
            assert!((lo..=hi).contains(&rank), "rank {rank} must be in its band");
            assert_eq!(NotificationPriority::from_rank(lo), level);
            assert_eq!(NotificationPriority::from_rank(hi), level);
        }
        // Bands partition 1–10 with no gaps or overlap.
        let mut bounds: Vec<(u8, u8)> = [
            NotificationPriority::Urgent,
            NotificationPriority::High,
            NotificationPriority::Medium,
            NotificationPriority::Low,
        ]
        .iter()
        .map(|l| l.rank_band())
        .collect();
        bounds.sort();
        assert_eq!(bounds[0].0, 1);
        assert_eq!(bounds[3].1, 10);
        for w in bounds.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0, "bands must be contiguous");
        }

        // Query-parameter form: case-insensitive, surrounding whitespace ok.
        assert_eq!(
            NotificationPriority::from_level_str("high"),
            Some(NotificationPriority::High)
        );
        assert_eq!(
            NotificationPriority::from_level_str("  URGENT "),
            Some(NotificationPriority::Urgent)
        );
        assert_eq!(
            NotificationPriority::from_level_str("Low"),
            Some(NotificationPriority::Low)
        );
        assert_eq!(
            NotificationPriority::from_level_str("medium"),
            Some(NotificationPriority::Medium)
        );
        assert_eq!(NotificationPriority::from_level_str("critical"), None);
    }

    #[test]
    fn default_priorities_match_hsi_table() {
        assert_eq!(
            NotificationType::AgentNeedsClarification.default_priority_rank(),
            1
        );
        assert_eq!(
            NotificationType::SpecApprovalRequested.default_priority_rank(),
            2
        );
        assert_eq!(NotificationType::GateFailure.default_priority_rank(), 3);
        assert_eq!(
            NotificationType::CrossWorkspaceSpecChange.default_priority_rank(),
            4
        );
        assert_eq!(
            NotificationType::SuggestedSpecLink.default_priority_rank(),
            10
        );
        assert_eq!(NotificationType::MrReverted.default_priority_rank(), 2);
        assert_eq!(
            NotificationType::MergeQueueEscalation.default_priority_rank(),
            1
        );
        assert_eq!(NotificationType::SecretExpiring.default_priority_rank(), 7);
        assert_eq!(
            NotificationType::PersonaChangePendingApproval.default_priority_rank(),
            2
        );
    }

    #[test]
    fn round_trip_type_strings() {
        let variants = [
            NotificationType::SpecApprovalRequested,
            NotificationType::PersonaApprovalRequested,
            NotificationType::AgentEscalation,
            NotificationType::AgentBudgetWarning,
            NotificationType::AgentBudgetExhausted,
            NotificationType::AgentFailed,
            NotificationType::GateFailure,
            NotificationType::GatePassed,
            NotificationType::MrMerged,
            NotificationType::MrReverted,
            NotificationType::MergeQueuePaused,
            NotificationType::BreakingChangeDetected,
            NotificationType::SpecDriftDetected,
            NotificationType::DependencyStale,
            NotificationType::SecurityFinding,
            NotificationType::InvitationReceived,
            NotificationType::MembershipChanged,
            NotificationType::SystemAlert,
            NotificationType::AgentNeedsClarification,
            NotificationType::CrossWorkspaceSpecChange,
            NotificationType::ConflictingInterpretations,
            NotificationType::MetaSpecDrift,
            NotificationType::TrustSuggestion,
            NotificationType::SpecAssertionFailure,
            NotificationType::AbandonedBranch,
            NotificationType::AgentCompleted,
            NotificationType::SuggestedSpecLink,
            NotificationType::SpecRejected,
            NotificationType::ConstraintViolation,
            NotificationType::CascadeTestTriggered,
            NotificationType::CascadeTestFailed,
            NotificationType::DependencyChainTooDeep,
            NotificationType::AtomicGroupFailure,
            NotificationType::SpecConflict,
            NotificationType::MergeQueueEscalation,
            NotificationType::PersonaApprovalRequested,
            NotificationType::MergeQueuePaused,
            NotificationType::SecurityFinding,
            NotificationType::SecretExpiring,
            NotificationType::PersonaChangePendingApproval,
        ];
        for v in &variants {
            assert_eq!(NotificationType::parse(v.as_str()).as_ref(), Some(v));
        }
    }

    #[test]
    fn new_notification_is_active_unread_and_workspace_sourced() {
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
        assert_eq!(n.priority_rank, 3);
        assert_eq!(n.priority, NotificationPriority::High);
        assert!(!n.read);
        assert!(n.read_at.is_none());
        assert_eq!(n.source.entity_type, "workspace");
        assert_eq!(n.source.entity_id, "ws1");
        assert_eq!(n.source.workspace_id, Id::new("ws1"));
        assert!(n.source.repo_id.is_none());
        assert!(n.action_url.is_none());
    }
}
