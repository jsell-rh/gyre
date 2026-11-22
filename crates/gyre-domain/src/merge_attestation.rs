//! Merge attestation bundle (agent-gates.md §Part 3 Cryptographic Gate
//! Attestation).
//!
//! When all gates pass and a merge executes, the forge produces a merge
//! attestation bundle — a signed JSON document attached to the merge
//! commit. The bundle carries the MR identity, the spec approval, the
//! author's agent identity, and one attestation per gate result, each
//! signed by the party the spec's "What Gets Signed and By Whom" table
//! names for that component: the forge signs TestCommand/LintCommand
//! results and the bundle itself; the gate agent signs its AgentReview
//! result. The result is a multi-party attestation where no single entity
//! can forge the complete bundle.
//!
//! These types are pure data. Signing orchestration lives behind the
//! `AttestationSigner` port (gyre-ports); the payload each signature
//! covers is the canonical JSON produced by `unsigned_canonical_json()` /
//! `unsigned_gate_json()`, so any implementation of the port (Ed25519
//! today, Sigstore/OIDC later) signs the same bytes.

use serde::{Deserialize, Serialize};

/// Version tag of the merge attestation bundle format (spec: "1.0").
pub const MERGE_ATTESTATION_VERSION: &str = "1.0";

/// Git notes ref for merge attestation bundles (agent-gates.md §Storage:
/// "as a git note on the merge commit", accessible via `git notes show`).
/// Distinct from the chain-attestation ref `refs/notes/attestations`
/// (supply-chain.md §5.3), whose dual-write logic owns that ref — a
/// separate ref avoids racing it.
pub const MERGE_ATTESTATION_NOTES_REF: &str = "refs/notes/merge-attestations";

/// Audit event type recorded for each stored merge attestation bundle
/// (agent-gates.md §Storage: "in the audit_events table for
/// queryability").
pub const MERGE_ATTESTATION_AUDIT_TYPE: &str = "merge_attestation_bundle";

/// SHA-256 of `bytes` as a `sha256:<hex>` string — the hash format the
/// spec uses for gate output hashes and review body hashes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

/// The signed merge attestation bundle attached to a merge commit
/// (agent-gates.md §Merge Attestation Bundle).
///
/// Serializes to the JSON schema in the spec: `merge_attestation_version`,
/// `mr_id`, `merge_commit_sha`, `merged_at`, `spec_ref`, `spec_approval`,
/// `author`, `gates[]`, `bundle_signature`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeAttestationBundle {
    /// Format version — always [`MERGE_ATTESTATION_VERSION`] ("1.0").
    pub merge_attestation_version: String,
    /// Id of the merged merge request.
    pub mr_id: String,
    /// SHA of the merge commit the bundle is attached to.
    pub merge_commit_sha: String,
    /// Unix epoch seconds when the merge was recorded.
    pub merged_at: u64,
    /// The MR's spec reference ("path@sha"), when one is bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_ref: Option<String>,
    /// Approval details for the referenced spec version, when an active
    /// approval exists in the ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_approval: Option<SpecRefAttestation>,
    /// The MR author's agent identity and provenance. `None` for MRs with
    /// no author agent (human-created MRs have no agent identity to
    /// attest — the field is omitted rather than fabricated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<AuthorAttestation>,
    /// One attestation per gate result recorded for the MR at merge time.
    pub gates: Vec<GateAttestation>,
    /// Forge signature over the canonical JSON of the entire document
    /// (with this field absent). `None` until the bundle is signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_signature: Option<String>,
}

impl MergeAttestationBundle {
    /// Canonical JSON of the unsigned document — the exact bytes the
    /// bundle-level forge signature covers. The signature field is
    /// excluded from its own payload.
    pub fn unsigned_canonical_json(&self) -> String {
        let mut unsigned = self.clone();
        unsigned.bundle_signature = None;
        serde_json::to_string(&unsigned).expect("bundle serialization must not fail")
    }
}

/// Spec approval details captured in the bundle (agent-gates.md: the
/// "spec_approval" object — approver, approval time, and the approver's
/// signature on the spec approval from the approval ledger).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecRefAttestation {
    /// Identity of the approver (user or agent), e.g. "user:jsell".
    pub approver: String,
    /// Unix epoch seconds when the spec version was approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<u64>,
    /// The approver's signature on the spec approval, when the ledger
    /// entry carries one (Sigstore signature on spec approval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl SpecRefAttestation {
    /// Canonical JSON of the unsigned spec approval attestation — the
    /// exact bytes a spec approval signature covers (the approver field
    /// and approval time, with the signature field excluded from its own
    /// payload), mirroring the bundle/gate canonical-form helpers so
    /// sign and verify share one payload definition.
    pub fn unsigned_spec_approval_json(&self) -> String {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        serde_json::to_string(&unsigned).expect("spec approval serialization must not fail")
    }
}

/// The MR author's agent identity and provenance (agent-gates.md: the
/// "author" object — agent identity, stack attestation, conversation SHA).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorAttestation {
    /// Agent id of the MR author.
    pub agent_id: String,
    /// OIDC subject of the author agent ("agent:<id>").
    pub oidc_sub: String,
    /// Stack attestation fingerprint from the author's workload
    /// attestation record, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_attestation: Option<String>,
    /// Identity-security.md / supply-chain.md §2 attestation level of the
    /// author's workload, derived from the workload attestation record
    /// with the canonical rule every provenance evaluation applies
    /// (0 = no workload attestation record, 1 = raw push without a stack
    /// fingerprint, 2 = CLI-attested stack, 3 = Gyre-managed container
    /// with stack + container + image) — never a bundle-local invention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation_level: Option<u32>,
    /// SHA-256 of the author's conversation blob (HSI §5 provenance),
    /// when the agent uploaded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_sha: Option<String>,
}

/// One gate result as attested in the bundle (agent-gates.md: the
/// "gates[]" entries — per-gate result, timing, signer identity, and
/// signature; AgentReview entries additionally carry the reviewer's
/// identity and review details).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateAttestation {
    /// Human-readable gate name.
    pub name: String,
    /// Gate type ("TestCommand", "LintCommand", "AgentReview", ...).
    #[serde(rename = "type")]
    pub gate_type: String,
    /// Gate status at merge time ("Passed", "Failed", ...).
    pub status: String,
    /// SHA-256 of the gate's captured output.
    pub output_hash: String,
    /// Unix epoch seconds when the gate started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// Unix epoch seconds when the gate finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    /// AgentReview/AgentValidation: id of the gate agent that reviewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_agent_id: Option<String>,
    /// AgentReview/AgentValidation: OIDC subject of the reviewer agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_oidc_sub: Option<String>,
    /// AgentReview/AgentValidation: the reviewer's stack attestation
    /// fingerprint, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_stack_attestation: Option<String>,
    /// AgentReview/AgentValidation: the reviewer's decision
    /// ("Approved"/"ChangesRequested").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<String>,
    /// AgentReview/AgentValidation: SHA-256 of the review body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_body_hash: Option<String>,
    /// Identity of the signer ("forge" or "agent:<id>") — proves which
    /// party attested this gate result.
    pub signed_by: String,
    /// The signer's signature over this gate attestation's canonical JSON
    /// (with this field absent). `None` until signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl GateAttestation {
    /// Canonical JSON of the unsigned gate attestation — the exact bytes
    /// the per-gate signature covers.
    pub fn unsigned_gate_json(&self) -> String {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        serde_json::to_string(&unsigned).expect("gate attestation serialization must not fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_gate(gate_type: &str, signed_by: &str) -> GateAttestation {
        GateAttestation {
            name: "tests".to_string(),
            gate_type: gate_type.to_string(),
            status: "Passed".to_string(),
            output_hash: "sha256:abcd".to_string(),
            started_at: Some(1_774_000_000),
            finished_at: Some(1_774_000_120),
            reviewer_agent_id: None,
            reviewer_oidc_sub: None,
            reviewer_stack_attestation: None,
            review_decision: None,
            review_body_hash: None,
            signed_by: signed_by.to_string(),
            signature: Some("<signature>".to_string()),
        }
    }

    #[test]
    fn bundle_serializes_to_spec_schema_field_names() {
        let bundle = MergeAttestationBundle {
            merge_attestation_version: MERGE_ATTESTATION_VERSION.to_string(),
            mr_id: "MR-042".to_string(),
            merge_commit_sha: "789abc".to_string(),
            merged_at: 1_774_000_000,
            spec_ref: Some("specs/system/identity-security.md@abc123".to_string()),
            spec_approval: Some(SpecRefAttestation {
                approver: "user:jsell".to_string(),
                approved_at: Some(1_774_000_000),
                signature: Some("<sigstore signature on spec approval>".to_string()),
            }),
            author: Some(AuthorAttestation {
                agent_id: "worker-42".to_string(),
                oidc_sub: "agent:worker-42".to_string(),
                stack_attestation: Some("sha256:stack-fingerprint".to_string()),
                attestation_level: Some(3),
                conversation_sha: Some("sha256:conversation".to_string()),
            }),
            gates: vec![sample_gate("TestCommand", "forge")],
            bundle_signature: Some("sig".to_string()),
        };

        let json: serde_json::Value = serde_json::to_value(&bundle).unwrap();
        // Every field the spec's JSON schema defines is present under the
        // spec's exact key names.
        for key in [
            "merge_attestation_version",
            "mr_id",
            "merge_commit_sha",
            "merged_at",
            "spec_ref",
            "spec_approval",
            "author",
            "gates",
            "bundle_signature",
        ] {
            assert!(json.get(key).is_some(), "missing spec field {key}");
        }
        assert_eq!(json["merge_attestation_version"], "1.0");
        let gate = &json["gates"][0];
        for key in [
            "name",
            "type",
            "status",
            "output_hash",
            "started_at",
            "finished_at",
            "signed_by",
            "signature",
        ] {
            assert!(gate.get(key).is_some(), "missing gate field {key}");
        }
        assert_eq!(gate["type"], "TestCommand");
        assert_eq!(json["spec_approval"]["approver"], "user:jsell");
        assert_eq!(json["author"]["oidc_sub"], "agent:worker-42");

        // Round-trips through the domain type.
        let back: MergeAttestationBundle = serde_json::from_value(json).unwrap();
        assert_eq!(back.mr_id, bundle.mr_id);
        assert_eq!(back.gates.len(), 1);
        assert_eq!(back.gates[0].gate_type, "TestCommand");
    }

    #[test]
    fn unsigned_canonical_json_excludes_signature() {
        let mut bundle = MergeAttestationBundle {
            merge_attestation_version: MERGE_ATTESTATION_VERSION.to_string(),
            mr_id: "MR-1".to_string(),
            merge_commit_sha: "abc".to_string(),
            merged_at: 1,
            spec_ref: None,
            spec_approval: None,
            author: None,
            gates: vec![],
            bundle_signature: None,
        };
        let unsigned = bundle.unsigned_canonical_json();
        assert!(!unsigned.contains("bundle_signature"));
        // The canonical form is deterministic: identical input, identical
        // bytes — required for signature verification parity.
        assert_eq!(unsigned, bundle.unsigned_canonical_json());

        bundle.bundle_signature = Some("sig".to_string());
        let signed_unsigned_form = bundle.unsigned_canonical_json();
        assert_eq!(unsigned, signed_unsigned_form);
        assert!(!signed_unsigned_form.contains("bundle_signature"));
    }

    #[test]
    fn gate_unsigned_json_excludes_signature() {
        let mut gate = sample_gate("AgentReview", "agent:security-gate-7");
        gate.signature = Some("sig".to_string());
        let unsigned = gate.unsigned_gate_json();
        assert!(!unsigned.contains("signature"));
        assert!(unsigned.contains("signed_by"));
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        // Fixed vector (sha256 of "gyre") so a broken hash wiring fails here.
        assert_eq!(
            sha256_hex(b"gyre"),
            "sha256:35a5b10bedb6b23b24f834e002cc768dcce64d75d527c98c309da4f6ad3a9ae2"
        );
        // Distinct inputs produce distinct hashes.
        assert_ne!(sha256_hex(b"gyre"), sha256_hex(b"gyre2"));
    }
}
