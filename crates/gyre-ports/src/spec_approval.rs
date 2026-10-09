use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::spec_approval::ApprovalTransitionError;
use gyre_domain::SpecApproval;

#[async_trait]
pub trait SpecApprovalRepository: Send + Sync {
    /// Insert a new ledger entry. Fails if an entry with the same id already
    /// exists (ids are freshly generated; a duplicate indicates a bug).
    /// Multiple approvals for the same spec path (different SHAs) are allowed.
    async fn create(&self, approval: &SpecApproval) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<SpecApproval>>;
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>>;
    /// Active approvals for a path: approved and not revoked/rejected.
    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>>;
    async fn list_all(&self) -> Result<Vec<SpecApproval>>;

    /// Transition an entry to Approved (Pending → Approved).
    /// Returns Ok(None) when the entry does not exist, Err on invalid transition.
    async fn approve(&self, id: &Id, now: u64) -> Result<Option<()>, ApprovalTransitionError>;

    /// Transition an entry to Revoked (Approved → Revoked).
    /// Revocation requires a non-empty reason and is audited by the caller.
    async fn revoke(
        &self,
        id: &Id,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError>;

    /// Transition an entry to Rejected (Pending → Rejected).
    async fn reject(
        &self,
        id: &Id,
        rejected_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError>;

    /// Revoke every active approval for a spec path (spec-file changed in a
    /// push: an approval is stale once the spec content changes).
    /// Returns the number of approvals invalidated.
    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<u64>;
}
