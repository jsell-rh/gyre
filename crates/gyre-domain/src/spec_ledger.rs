//! Spec registry ledger domain types.

use serde::{Deserialize, Serialize};

/// Approval status of a spec in the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Deprecated,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalStatus::Pending => write!(f, "pending"),
            ApprovalStatus::Approved => write!(f, "approved"),
            ApprovalStatus::Deprecated => write!(f, "deprecated"),
        }
    }
}

/// A single ledger entry tracking runtime state for one spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecLedgerEntry {
    pub path: String,
    pub title: String,
    pub owner: String,
    /// Git blob SHA of the spec file at HEAD.
    pub current_sha: String,
    /// Approval mode from the manifest.
    pub approval_mode: String,
    /// Current approval status.
    pub approval_status: ApprovalStatus,
    /// Task IDs associated with this spec.
    pub linked_tasks: Vec<String>,
    /// MR IDs referencing this spec via spec_ref.
    pub linked_mrs: Vec<String>,
    /// Drift status: "clean", "drifted", "unknown".
    pub drift_status: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// An event in the approval history for a spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecApprovalEvent {
    pub id: String,
    pub spec_path: String,
    /// The blob SHA that was approved.
    pub spec_sha: String,
    /// "human" or "agent".
    pub approver_type: String,
    /// Identity string, e.g. "user:jsell" or "agent:<uuid>".
    pub approver_id: String,
    /// Agent persona name (null for human approvers).
    pub persona: Option<String>,
    pub approved_at: u64,
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
}

impl SpecApprovalEvent {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}
