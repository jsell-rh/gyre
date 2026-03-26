//! Merge attestation domain types.

use gyre_common::AgentCompletionSummary;
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
    /// SHA-256 of the agent's conversation blob (HSI §5 provenance).
    /// Populated from the KV store at merge time; None if the agent did not upload a conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_sha: Option<String>,
    /// Agent completion summary (HSI §4) — populated when the agent calls `agent.complete`
    /// with a `summary` field. Contains decisions, uncertainties, and conversation_sha.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_summary: Option<AgentCompletionSummary>,
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
