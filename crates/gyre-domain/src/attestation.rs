//! Merge attestation domain types.

use serde::{Deserialize, Serialize};

/// Snapshot of a single gate result captured at merge time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationGateResult {
    pub gate_id: String,
    pub gate_type: String,
    pub status: String,
    pub output: Option<String>,
}

/// The attestation payload for one merge event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeAttestation {
    /// Gyre attestation format version.
    pub attestation_version: u32,
    pub mr_id: String,
    pub merge_commit_sha: String,
    /// Unix epoch seconds when the merge was recorded.
    pub merged_at: u64,
    /// Gate results at the time of merge.
    pub gate_results: Vec<AttestationGateResult>,
    /// Spec reference bound to this MR, if any.
    pub spec_ref: Option<String>,
    /// Whether every referenced spec had an active approval at merge time.
    pub spec_fully_approved: bool,
    /// Agent ID of the MR author.
    pub author_agent_id: Option<String>,
}

/// Signed attestation bundle returned by `GET /api/v1/merge-requests/{id}/attestation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationBundle {
    pub attestation: MergeAttestation,
    /// Base64-encoded Ed25519 signature over the canonical JSON of `attestation`.
    pub signature: String,
    /// `kid` of the Ed25519 key that produced `signature`.
    pub signing_key_id: String,
}
