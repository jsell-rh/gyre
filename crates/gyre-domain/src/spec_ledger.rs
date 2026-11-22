//! Spec registry ledger domain types.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Approval status of a spec in the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Deprecated,
    /// Approval was explicitly revoked (approval withdrawn after being granted).
    Revoked,
    /// Spec was rejected by a human reviewer (never approved).
    Rejected,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalStatus::Pending => write!(f, "pending"),
            ApprovalStatus::Approved => write!(f, "approved"),
            ApprovalStatus::Deprecated => write!(f, "deprecated"),
            ApprovalStatus::Revoked => write!(f, "revoked"),
            ApprovalStatus::Rejected => write!(f, "rejected"),
        }
    }
}

/// A single ledger entry tracking runtime state for one spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecLedgerEntry {
    pub path: String,
    pub title: String,
    pub owner: String,
    /// Optional spec kind, e.g. "meta:persona", "meta:principle", "meta:standard", "meta:process".
    pub kind: Option<String>,
    /// Git blob SHA of the spec file at HEAD.
    pub current_sha: String,
    /// SHA-256 over the spec's meta-spec binding set (canonical form:
    /// one "{meta_spec_id}\t{pinned_version}" line per binding, sorted).
    /// Empty when the spec has no bindings — the binding set is spec-registry
    /// metadata covered by the registry SHA the approval ledger records
    /// (agent-runtime.md §2 Spec-Level Binding), so changing a pin changes
    /// the spec's registry identity and requires re-approval.
    #[serde(default)]
    pub bindings_sha: String,
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
    /// Repo that owns this spec (for SpecApproved signal chain routing).
    pub repo_id: Option<String>,
    /// Workspace scope (for SpecApproved Destination::Workspace routing).
    pub workspace_id: Option<String>,
}

impl SpecLedgerEntry {
    /// The registry SHA of the spec: the identity the approval ledger
    /// records and validates against. Covers the spec's content (`current_sha`,
    /// the git blob SHA of the markdown file) AND its meta-spec binding set
    /// (`bindings_sha`) — agent-runtime.md §2 Spec-Level Binding stores
    /// bindings "in the spec registry alongside other spec metadata", and
    /// §2 Stale Pin Detection requires "Human updates the pin → spec
    /// metadata changes → spec SHA changes → spec needs re-approval".
    ///
    /// Equals `current_sha` when the spec has no bindings, so every spec
    /// without a binding set keeps its existing identity (recorded approvals
    /// stay valid). With bindings, a new composite SHA is derived over both
    /// parts: any content edit or binding/pin change produces a new registry
    /// SHA and invalidates prior approvals via the existing staleness
    /// machinery (approval events are valid only for the SHA they were
    /// granted on).
    pub fn registry_sha(&self) -> String {
        if self.bindings_sha.is_empty() {
            return self.current_sha.clone();
        }
        let mut hasher = Sha256::new();
        hasher.update(self.current_sha.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.bindings_sha.as_bytes());
        format!("{:x}", hasher.finalize())
    }
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
    /// Agent attestation level (0-3) captured from the approving agent's
    /// verified JWT workload claims at approval time (null for humans).
    /// Required to evaluate §9 agent-approval validity.
    pub attestation_level: Option<u32>,
    /// Agent stack fingerprint captured from the approving agent's verified
    /// JWT `wl_stack_hash` claim at approval time (null for humans).
    /// Required to evaluate §9 stack_hash matching.
    pub stack_hash: Option<String>,
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
