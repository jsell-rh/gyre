//! Spec approval ledger: cryptographic binding of specs to code at merge time.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A recorded approval of a spec at a specific git blob SHA.
///
/// Approvals are immutable — once recorded, they can be revoked but not modified.
/// Multiple approvals can exist for the same spec path (different versions).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecApproval {
    pub id: Id,
    /// Relative path to the spec file, e.g. "specs/system/agent-gates.md".
    pub spec_path: String,
    /// Git blob SHA of the spec file at approval time (40-char hex).
    pub spec_sha: String,
    /// Identity of the approver (user or agent), e.g. "user:jsell" or "agent:<uuid>".
    pub approver_id: String,
    /// Optional Sigstore signature for cryptographic proof of approval.
    pub signature: Option<String>,
    pub approved_at: u64,
    /// When this approval was revoked (None = still active).
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    /// When this approval was rejected by a human reviewer.
    pub rejected_at: Option<u64>,
    pub rejected_reason: Option<String>,
    pub rejected_by: Option<Id>,
}

impl SpecApproval {
    pub fn new(
        id: Id,
        spec_path: impl Into<String>,
        spec_sha: impl Into<String>,
        approver_id: impl Into<String>,
        approved_at: u64,
    ) -> Self {
        Self {
            id,
            spec_path: spec_path.into(),
            spec_sha: spec_sha.into(),
            approver_id: approver_id.into(),
            signature: None,
            approved_at,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
            rejected_at: None,
            rejected_reason: None,
            rejected_by: None,
        }
    }

    /// Returns true if this approval is still active (not revoked or rejected).
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none() && self.rejected_at.is_none()
    }

    /// Reject this approval with a reason and actor.
    pub fn reject(&mut self, reason: impl Into<String>, by: Id, now: u64) {
        self.rejected_at = Some(now);
        self.rejected_reason = Some(reason.into());
        self.rejected_by = Some(by);
    }
}
