//! Spec approval ledger: cryptographic binding of specs to code at merge time.
//!
//! agent-gates.md §Spec Approval Ledger: status is *derived* from which
//! timestamp column is non-null, never stored directly. Only one of
//! approved_at / revoked_at / rejected_at is non-null at any time
//! (mutual exclusivity, maintained on transition).

use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Lifecycle status of a spec approval, derived from the timestamp columns.
///
/// agent-gates.md §Spec Approval Ledger:
/// - Pending  — no timestamp columns set
/// - Approved — approved_at is set
/// - Revoked  — revoked_at is set (post-merge withdrawal)
/// - Rejected — rejected_at is set (pre-merge decline)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Revoked,
    Rejected,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalStatus::Pending => write!(f, "pending"),
            ApprovalStatus::Approved => write!(f, "approved"),
            ApprovalStatus::Revoked => write!(f, "revoked"),
            ApprovalStatus::Rejected => write!(f, "rejected"),
        }
    }
}

/// A recorded approval of a spec at a specific git blob SHA.
///
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
    /// When this approval was approved (None = status Pending, per spec schema).
    pub approved_at: Option<u64>,
    /// When this approval was revoked (None = not revoked).
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    /// When this approval was rejected by a human reviewer (pre-merge decline).
    pub rejected_at: Option<u64>,
    pub rejected_reason: Option<String>,
    pub rejected_by: Option<String>,
}

impl SpecApproval {
    /// Create a new Pending approval ledger entry (no timestamps set).
    pub fn new(
        id: Id,
        spec_path: impl Into<String>,
        spec_sha: impl Into<String>,
        approver_id: impl Into<String>,
    ) -> Self {
        Self {
            id,
            spec_path: spec_path.into(),
            spec_sha: spec_sha.into(),
            approver_id: approver_id.into(),
            signature: None,
            approved_at: None,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
            rejected_at: None,
            rejected_reason: None,
            rejected_by: None,
        }
    }

    /// Derive the lifecycle status from which timestamp column is non-null
    /// (agent-gates.md: "Status is derived from which timestamp column is
    /// non-null"). Revoked/Rejected win over Approved so a corrupted row with
    /// two timestamps still reports the terminal state.
    pub fn status(&self) -> ApprovalStatus {
        if self.revoked_at.is_some() {
            ApprovalStatus::Revoked
        } else if self.rejected_at.is_some() {
            ApprovalStatus::Rejected
        } else if self.approved_at.is_some() {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        }
    }

    /// Returns true if this approval authoritatively approves the spec SHA
    /// (approved and not since withdrawn or declined).
    pub fn is_active(&self) -> bool {
        self.approved_at.is_some() && self.revoked_at.is_none() && self.rejected_at.is_none()
    }

    /// Transition Pending → Approved.
    /// Fails if the approval is not Pending (spec: valid transitions are
    /// Pending → Approved → Revoked and Pending → Rejected).
    pub fn approve(&mut self, now: u64) -> Result<(), ApprovalTransitionError> {
        if self.status() != ApprovalStatus::Pending {
            return Err(ApprovalTransitionError::new(
                self.status(),
                ApprovalStatus::Approved,
            ));
        }
        self.approved_at = Some(now);
        Ok(())
    }

    /// Transition Approved → Revoked (post-merge withdrawal).
    /// Revocation requires a reason (spec) and clears the other timestamp
    /// columns to maintain mutual exclusivity.
    pub fn revoke(
        &mut self,
        revoked_by: impl Into<String>,
        reason: &str,
        now: u64,
    ) -> Result<(), ApprovalTransitionError> {
        if reason.trim().is_empty() {
            return Err(ApprovalTransitionError::InvalidRevocation(
                "revocation requires a reason".to_string(),
            ));
        }
        if self.status() != ApprovalStatus::Approved {
            return Err(ApprovalTransitionError::new(
                self.status(),
                ApprovalStatus::Revoked,
            ));
        }
        // Mutual exclusivity: clear other timestamp columns.
        self.approved_at = None;
        self.rejected_at = None;
        self.rejected_by = None;
        self.rejected_reason = None;
        self.revoked_at = Some(now);
        self.revoked_by = Some(revoked_by.into());
        self.revocation_reason = Some(reason.to_string());
        Ok(())
    }

    /// Transition Pending → Rejected (pre-merge decline).
    /// Clears the other timestamp columns to maintain mutual exclusivity.
    pub fn reject(
        &mut self,
        rejected_by: impl Into<String>,
        reason: &str,
        now: u64,
    ) -> Result<(), ApprovalTransitionError> {
        if self.status() != ApprovalStatus::Pending {
            return Err(ApprovalTransitionError::new(
                self.status(),
                ApprovalStatus::Rejected,
            ));
        }
        // Mutual exclusivity: clear other timestamp columns.
        self.approved_at = None;
        self.revoked_at = None;
        self.revoked_by = None;
        self.revocation_reason = None;
        self.rejected_at = Some(now);
        self.rejected_by = Some(rejected_by.into());
        self.rejected_reason = Some(reason.to_string());
        Ok(())
    }
}

/// A requested status transition violates the ledger's transition rules.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApprovalTransitionError {
    #[error("invalid transition: {from} → {to}")]
    InvalidTransition {
        from: ApprovalStatus,
        to: ApprovalStatus,
    },
    #[error("{0}")]
    InvalidRevocation(String),
    /// Storage failure while loading/persisting the approval row.
    #[error("storage failure: {0}")]
    Storage(String),
}

impl ApprovalTransitionError {
    fn new(from: ApprovalStatus, to: ApprovalStatus) -> Self {
        Self::InvalidTransition { from, to }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval() -> SpecApproval {
        SpecApproval::new(
            Id::new("apr_test"),
            "specs/system/agent-gates.md",
            "a".repeat(40),
            "user:jsell",
        )
    }
    #[test]
    fn status_is_derived_from_timestamps() {
        let mut a = approval();
        assert_eq!(a.status(), ApprovalStatus::Pending);
        assert!(!a.is_active());

        a.approve(100).unwrap();
        assert_eq!(a.status(), ApprovalStatus::Approved);
        assert!(a.is_active());

        a.revoke("user:admin", "spec withdrawn", 200).unwrap();
        assert_eq!(a.status(), ApprovalStatus::Revoked);
        assert!(!a.is_active());
        // Mutual exclusivity: approved_at cleared on transition.
        assert_eq!(a.approved_at, None);
        assert_eq!(a.revoked_at, Some(200));
        assert_eq!(a.revoked_by.as_deref(), Some("user:admin"));
        assert_eq!(a.revocation_reason.as_deref(), Some("spec withdrawn"));
    }

    #[test]
    fn pending_to_rejected() {
        let mut a = approval();
        a.reject("user:reviewer", "not ready", 100).unwrap();
        assert_eq!(a.status(), ApprovalStatus::Rejected);
        assert!(!a.is_active());
        assert_eq!(a.approved_at, None);
        assert_eq!(a.rejected_at, Some(100));
        assert_eq!(a.rejected_by.as_deref(), Some("user:reviewer"));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        // Pending cannot be revoked directly (Pending → Approved → Revoked).
        let mut a = approval();
        assert!(a.revoke("user:admin", "reason", 100).is_err());

        // Pending cannot be rejected twice.
        let mut a = approval();
        a.reject("user:reviewer", "no", 100).unwrap();
        assert!(a.reject("user:reviewer", "no", 110).is_err());

        // Approved cannot be rejected (only revoked).
        let mut a = approval();
        a.approve(100).unwrap();
        assert!(a.reject("user:reviewer", "no", 110).is_err());

        // Approved cannot be re-approved.
        let mut a = approval();
        a.approve(100).unwrap();
        assert!(a.approve(110).is_err());

        // Revoked is terminal.
        let mut a = approval();
        a.approve(100).unwrap();
        a.revoke("user:admin", "reason", 200).unwrap();
        assert!(a.approve(300).is_err());
        assert!(a.revoke("user:admin", "reason", 300).is_err());
    }

    #[test]
    fn revocation_requires_reason() {
        let mut a = approval();
        a.approve(100).unwrap();
        assert!(a.revoke("user:admin", "", 200).is_err());
        assert!(a.revoke("user:admin", "   ", 200).is_err());
        // State unchanged after failed transition.
        assert_eq!(a.status(), ApprovalStatus::Approved);
    }
}
