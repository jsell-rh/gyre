use gyre_common::Id;
use serde::{Deserialize, Serialize};

// ─── User Notification Preferences ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserNotificationPreference {
    pub user_id: Id,
    pub notification_type: String,
    pub enabled: bool,
}

impl UserNotificationPreference {
    pub fn new(user_id: Id, notification_type: impl Into<String>, enabled: bool) -> Self {
        Self {
            user_id,
            notification_type: notification_type.into(),
            enabled,
        }
    }
}

// ─── User Tokens ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserToken {
    pub id: Id,
    pub user_id: Id,
    pub name: String,
    pub token_hash: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub expires_at: Option<u64>,
}

impl UserToken {
    pub fn new(
        id: Id,
        user_id: Id,
        name: impl Into<String>,
        token_hash: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            user_id,
            name: name.into(),
            token_hash: token_hash.into(),
            created_at,
            last_used_at: None,
            expires_at: None,
        }
    }
}

// ─── Judgment Ledger ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JudgmentType {
    /// spec_approvals table — spec approval
    SpecApproval,
    /// spec_approvals table — spec rejection (revocation)
    SpecRejection,
    /// workspace audit log — trust grant
    TrustGrant,
    /// workspace audit log — meta-spec update
    MetaSpec,
}

impl JudgmentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            JudgmentType::SpecApproval => "approval",
            JudgmentType::SpecRejection => "rejection",
            JudgmentType::TrustGrant => "trust",
            JudgmentType::MetaSpec => "meta-spec",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "approval" => Some(JudgmentType::SpecApproval),
            "rejection" => Some(JudgmentType::SpecRejection),
            "trust" => Some(JudgmentType::TrustGrant),
            "meta-spec" => Some(JudgmentType::MetaSpec),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentEntry {
    pub judgment_type: JudgmentType,
    /// e.g. spec path, workspace name
    pub entity_ref: String,
    pub workspace_id: Option<Id>,
    pub timestamp: u64,
    pub detail: Option<String>,
}

impl JudgmentEntry {
    pub fn new(
        judgment_type: JudgmentType,
        entity_ref: impl Into<String>,
        workspace_id: Option<Id>,
        timestamp: u64,
        detail: Option<String>,
    ) -> Self {
        Self {
            judgment_type,
            entity_ref: entity_ref.into(),
            workspace_id,
            timestamp,
            detail,
        }
    }
}
