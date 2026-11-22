//! Merge attestation bundle storage (agent-gates.md §Part 3 Storage).
//!
//! The spec names four storage targets for every merge attestation
//! bundle: a git note on the merge commit, the `audit_events` table,
//! the Rekor transparency log (published through the separate
//! [`crate::rekor::RekorClient`] port), and the AIBOM of each release
//! that includes the merge commit (fed by the bulk query on this
//! port). This port is the seam the merge processor stores and
//! retrieves bundles through, so storage targets can evolve without
//! touching the merge flow.

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::merge_attestation::MergeAttestationBundle;

/// Scope of the merge a bundle belongs to. The audit-event record is
/// filed under this workspace/repo identity so scoped audit queries
/// (ABAC) see the bundle.
pub struct AttestationScope {
    /// Workspace the merged MR belongs to.
    pub workspace_id: Id,
    /// Repository the merge landed in.
    pub repo_id: Id,
}

/// Storage and retrieval of merge attestation bundles
/// (agent-gates.md §Part 3 Storage).
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait AttestationStorage: Send + Sync {
    /// Persist `bundle` to the storage targets that record a merge the
    /// moment it lands: a git note on the merge commit (`repo_path` is
    /// the on-disk git repository the merge landed in) and a
    /// `merge_attestation_bundle` audit event carrying the full signed
    /// bundle JSON, indexed by MR id. Rekor transparency-log publication
    /// is a separate port ([`crate::rekor::RekorClient`]).
    async fn store(
        &self,
        repo_path: &Path,
        scope: &AttestationScope,
        bundle: &MergeAttestationBundle,
    ) -> Result<()>;

    /// The bundle attached to `commit_sha`. Served from the git note on
    /// the commit when present; otherwise from the audit-event record.
    async fn retrieve_by_commit(
        &self,
        repo_path: &Path,
        commit_sha: &str,
    ) -> Result<Option<MergeAttestationBundle>>;

    /// The newest bundle generated for the merge request `mr_id`, from
    /// the audit-event record.
    async fn retrieve_by_mr(&self, mr_id: &Id) -> Result<Option<MergeAttestationBundle>>;

    /// Every bundle attached to the given commit SHAs — the AIBOM
    /// integration point (agent-gates.md §Storage: \"in the AIBOM for
    /// each release that includes the merge commit\"): release AIBOM
    /// generation collects all bundles for the commits the release
    /// includes. Served from the audit-event records.
    async fn retrieve_by_commits(
        &self,
        commit_shas: &[String],
    ) -> Result<Vec<MergeAttestationBundle>>;
}
