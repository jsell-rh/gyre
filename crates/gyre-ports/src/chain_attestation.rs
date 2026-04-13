//! Port trait for authorization provenance attestation chain persistence (§5.4).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Attestation;

/// Repository for storing and querying attestation chain nodes.
///
/// Attestations are content-addressable (keyed by hash). The chain is walked
/// via `parent_ref` in `DerivedInput` nodes. Implementations must support
/// efficient chain traversal for verification.
#[async_trait]
pub trait ChainAttestationRepository: Send + Sync {
    /// Store an attestation node.
    async fn save(&self, attestation: &Attestation) -> Result<()>;

    /// Load by content-addressable ID.
    async fn find_by_id(&self, id: &str) -> Result<Option<Attestation>>;

    /// Load the chain rooted at this attestation (walks parent_ref).
    /// Returns attestation nodes from root to leaf.
    async fn load_chain(&self, leaf_id: &str) -> Result<Vec<Attestation>>;

    /// Find attestations for a task.
    async fn find_by_task(&self, task_id: &str) -> Result<Vec<Attestation>>;

    /// Find the attestation for a specific commit.
    async fn find_by_commit(&self, commit_sha: &str) -> Result<Option<Attestation>>;

    /// Find attestations for a repo within a time range (Unix epoch seconds).
    async fn find_by_repo(&self, repo_id: &str, since: u64, until: u64)
        -> Result<Vec<Attestation>>;
}
