//! Verification protocol for merge attestation bundles
//! (agent-gates.md §Part 3 Verification).
//!
//! Anyone who trusts Gyre's OIDC issuer can verify an entire bundle:
//!
//! 1. Verify `bundle_signature` against Gyre's OIDC issuer.
//! 2. For each gate, verify its signature against the signer's identity.
//! 3. Verify the spec approval signature.
//! 4. Verify the author's commit signatures via Rekor (deferred to the
//!    Sigstore/Rekor integration — the bundle carries no author commit
//!    SHAs to check; see `crates/gyre-server/src/merge_attestation.rs`).
//! 5. Check that all gate results reference the same MR and spec.
//!
//! This module holds the protocol's *result* types and the pure,
//! I/O-free part of step 5: structural consistency checks over a
//! bundle. Cryptographic verification (steps 1–3) is delegated to the
//! `AttestationVerifier` port (`gyre-ports`) — an infrastructure
//! concern; the server-side `ForgeAttestationVerifier` combines both
//! halves into a [`VerificationResult`].
//!
//! Consistency model for step 5: in the bundle format (§Merge
//! Attestation Bundle) gate results do not carry per-gate `mr_id` or
//! spec fields — they are attested *inside* the bundle, whose single
//! `mr_id` and `spec_ref` ("path@sha") bind every gate to one MR and at
//! most one spec version by construction. The consistency check
//! therefore enforces the invariants that make that binding sound:
//! non-empty `mr_id`/`merge_commit_sha`, a well-formed `spec_ref`, a
//! `spec_approval` only when a `spec_ref` exists, per-gate signature
//! presence and a `signed_by` identity that matches the gate type's
//! signing party and the recorded reviewer identity, and an author
//! block whose OIDC subject matches its agent id.

use serde::{Deserialize, Serialize};

use crate::merge_attestation::{MergeAttestationBundle, MERGE_ATTESTATION_VERSION};

/// Whether a gate type's results are signed by the gate agent that
/// produced the review (agent-gates.md §What Gets Signed and By Whom:
/// "AgentReview results — Gate agent"). All other gate types are signed
/// by the forge.
pub fn is_agent_gate_type(gate_type: &str) -> bool {
    matches!(gate_type, "AgentReview" | "AgentValidation")
}

/// Per-gate outcome of bundle verification: the gate's identity, whether
/// its signature verified, and any errors recorded for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateVerificationResult {
    /// Human-readable gate name (from the attestation).
    pub name: String,
    /// Gate type ("TestCommand", "AgentReview", ...).
    pub gate_type: String,
    /// Identity the attestation names as its signer ("forge" or
    /// "agent:<id>").
    pub signed_by: String,
    /// Whether the gate's signature verified against the named party's
    /// key. `false` when the attestation carries no signature.
    pub signature_valid: bool,
    /// Errors recorded for this gate (missing signature, unknown signer
    /// identity, failed verification).
    pub errors: Vec<String>,
}

/// Full outcome of verifying a merge attestation bundle
/// (agent-gates.md §Verification): one flag per verification step plus
/// the collected errors, so an auditor sees *which* component failed
/// rather than a single boolean.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Overall verdict: every verified component passed and the bundle
    /// is structurally consistent. `spec_approval_valid == None` (no
    /// spec approval signature to verify) does not fail the bundle —
    /// the ledger's signature is optional.
    pub valid: bool,
    /// Step 1: the bundle-level forge signature over the canonical
    /// unsigned document verified against the issuer's key.
    pub bundle_signature_valid: bool,
    /// Step 2: one entry per gate attestation, in bundle order.
    pub gate_results: Vec<GateVerificationResult>,
    /// Step 3: the spec approval signature's verification outcome.
    /// `None` when the bundle carries no spec approval, or the approval
    /// carries no signature (nothing cryptographic to verify — reported
    /// in `errors`, never silently treated as passed).
    pub spec_approval_valid: Option<bool>,
    /// Step 5: all gate results reference the same MR and spec — the
    /// bundle's structural consistency checks passed.
    pub consistency_check: bool,
    /// Every error found, across all steps.
    pub errors: Vec<String>,
}

/// Pure structural consistency check (agent-gates.md §Verification
/// step 5): verifies that the bundle's gate results coherently
/// reference a single MR and spec. Returns the list of consistency
/// errors — empty means consistent. No I/O, no crypto: signature
/// *validity* is the verifier port's concern; this checks only the
/// structural invariants the signatures are evaluated against.
pub fn check_bundle_consistency(bundle: &MergeAttestationBundle) -> Vec<String> {
    let mut errors = Vec::new();

    if bundle.merge_attestation_version != MERGE_ATTESTATION_VERSION {
        errors.push(format!(
            "bundle merge_attestation_version is {:?}, expected {MERGE_ATTESTATION_VERSION:?}",
            bundle.merge_attestation_version
        ));
    }
    if bundle.mr_id.is_empty() {
        errors.push(
            "bundle mr_id is empty — gate results cannot be tied to a merge request".to_string(),
        );
    }
    if bundle.merge_commit_sha.is_empty() {
        errors.push("bundle merge_commit_sha is empty".to_string());
    }

    for gate in &bundle.gates {
        let label = format!("gate {:?}", gate.name);
        if gate.signature.is_none() {
            errors.push(format!("{label} carries no signature"));
        }
        match gate.signed_by.strip_prefix("agent:") {
            Some("") => {
                errors.push(format!("{label} signed_by \"agent:\" has no agent id"));
            }
            Some(_) => {
                if !is_agent_gate_type(&gate.gate_type) {
                    errors.push(format!(
                        "{label} is a {} gate signed by an agent — command-run gate results are signed by the forge",
                        gate.gate_type
                    ));
                }
                if let Some(reviewer) = gate.reviewer_agent_id.as_deref() {
                    if gate.signed_by != format!("agent:{reviewer}") {
                        errors.push(format!(
                            "{label} is signed by {} but records reviewer {reviewer} — the reviewer must be the signer",
                            gate.signed_by
                        ));
                    }
                } else if is_agent_gate_type(&gate.gate_type) {
                    errors.push(format!(
                        "{label} is an {} gate with no reviewer_agent_id — the signing gate agent must be recorded",
                        gate.gate_type
                    ));
                }
                if let Some(oidc_sub) = gate.reviewer_oidc_sub.as_deref() {
                    let expected =
                        format!("agent:{}", gate.reviewer_agent_id.as_deref().unwrap_or(""));
                    if oidc_sub != expected {
                        errors.push(format!(
                            "{label} reviewer_oidc_sub {oidc_sub:?} does not match the recorded reviewer ({expected:?})"
                        ));
                    }
                }
            }
            None if gate.signed_by != "forge" => {
                errors.push(format!(
                    "{label} signed_by {:?} names neither \"forge\" nor \"agent:<id>\"",
                    gate.signed_by
                ));
            }
            None => {
                // signed_by == "forge": agent review/validation results
                // are signed by the gate agent that produced them
                // (agent-gates.md §What Gets Signed and By Whom) — a
                // forge self-attestation of an agent gate would let a
                // single party forge the multi-party bundle.
                if is_agent_gate_type(&gate.gate_type) {
                    errors.push(format!(
                        "{label} is an {} gate signed by the forge — agent review results are signed by the gate agent that produced them",
                        gate.gate_type
                    ));
                }
            }
        }
    }

    match bundle.spec_ref.as_deref() {
        Some(spec_ref) => {
            let well_formed = spec_ref
                .rsplit_once('@')
                .map(|(path, sha)| !path.is_empty() && !sha.is_empty())
                .unwrap_or(false);
            if !well_formed {
                errors.push(format!(
                    "bundle spec_ref {spec_ref:?} is not a well-formed \"path@sha\" reference — gate results cannot be tied to one spec version"
                ));
            }
        }
        None => {
            if bundle.spec_approval.is_some() {
                errors.push(
                    "bundle carries a spec_approval without a spec_ref — an approval must reference the spec version it approves"
                        .to_string(),
                );
            }
        }
    }

    if let Some(author) = &bundle.author {
        let expected_sub = format!("agent:{}", author.agent_id);
        if author.oidc_sub != expected_sub {
            errors.push(format!(
                "author oidc_sub {:?} does not match agent id {:?} (expected {expected_sub:?})",
                author.oidc_sub, author.agent_id
            ));
        }
    }

    errors
}

/// Assemble the final [`VerificationResult`] from the per-component
/// outcomes (pure — the combination rule, kept out of any one adapter so
/// every verifier implementation scores components identically).
///
/// `valid` requires: the bundle signature verified, every gate
/// signature verified, the consistency check passed, and the spec
/// approval signature — when one exists to verify — verified. A missing
/// spec-approval signature (`spec_approval_valid == None`) is reported
/// in `errors` but does not fail the bundle: the approval ledger's
/// signature is optional.
pub fn assemble_verification_result(
    bundle_signature_valid: bool,
    gate_results: Vec<GateVerificationResult>,
    spec_approval_valid: Option<bool>,
    consistency_errors: Vec<String>,
) -> VerificationResult {
    let consistency_check = consistency_errors.is_empty();
    let mut errors = consistency_errors;
    if !bundle_signature_valid {
        errors.push("bundle_signature failed verification against the issuer's key".to_string());
    }
    for gate in &gate_results {
        if !gate.signature_valid {
            errors.push(format!(
                "gate {:?} ({}) signature failed verification",
                gate.name, gate.signed_by
            ));
        }
        for err in &gate.errors {
            errors.push(format!("gate {:?}: {err}", gate.name));
        }
    }
    if spec_approval_valid == Some(false) {
        errors.push("spec approval signature failed verification".to_string());
    }
    let valid = bundle_signature_valid
        && consistency_check
        && gate_results.iter().all(|g| g.signature_valid)
        && spec_approval_valid.unwrap_or(true);
    VerificationResult {
        valid,
        bundle_signature_valid,
        gate_results,
        spec_approval_valid,
        consistency_check,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge_attestation::{AuthorAttestation, GateAttestation, SpecRefAttestation};

    fn sample_gate(gate_type: &str, signed_by: &str) -> GateAttestation {
        let agent_gate = is_agent_gate_type(gate_type);
        GateAttestation {
            name: format!("{gate_type}-gate"),
            gate_type: gate_type.to_string(),
            status: "Passed".to_string(),
            output_hash: "sha256:abcd".to_string(),
            started_at: Some(1),
            finished_at: Some(2),
            reviewer_agent_id: agent_gate.then(|| "security-gate-7".to_string()),
            reviewer_oidc_sub: agent_gate.then(|| "agent:security-gate-7".to_string()),
            reviewer_stack_attestation: None,
            review_decision: agent_gate.then(|| "Approved".to_string()),
            review_body_hash: None,
            signed_by: signed_by.to_string(),
            signature: Some("sig".to_string()),
        }
    }

    fn sample_bundle() -> MergeAttestationBundle {
        MergeAttestationBundle {
            merge_attestation_version: MERGE_ATTESTATION_VERSION.to_string(),
            mr_id: "MR-042".to_string(),
            merge_commit_sha: "789abc".to_string(),
            merged_at: 1_774_000_000,
            spec_ref: Some("specs/system/agent-gates.md@abc123".to_string()),
            spec_approval: Some(SpecRefAttestation {
                approver: "user:jsell".to_string(),
                approved_at: Some(1_774_000_000),
                signature: None,
            }),
            author: Some(AuthorAttestation {
                agent_id: "worker-42".to_string(),
                oidc_sub: "agent:worker-42".to_string(),
                stack_attestation: Some("sha256:stack".to_string()),
                attestation_level: Some(3),
                conversation_sha: Some("sha256:conversation".to_string()),
            }),
            gates: vec![
                sample_gate("TestCommand", "forge"),
                sample_gate("AgentReview", "agent:security-gate-7"),
            ],
            bundle_signature: Some("sig".to_string()),
        }
    }

    #[test]
    fn consistent_bundle_passes_consistency_check() {
        assert!(
            check_bundle_consistency(&sample_bundle()).is_empty(),
            "a well-formed bundle must be consistent"
        );
    }

    #[test]
    fn empty_mr_id_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.mr_id = String::new();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors.iter().any(|e| e.contains("mr_id is empty")),
            "got {errors:?}"
        );
    }

    #[test]
    fn agent_gate_signed_by_mismatched_reviewer_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.gates[1].signed_by = "agent:someone-else".to_string();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("records reviewer security-gate-7")),
            "got {errors:?}"
        );
    }

    #[test]
    fn agent_gate_signed_by_forge_fails_consistency() {
        // The signing-party rule runs in both directions: an
        // AgentReview/AgentValidation gate attestation signed by "forge"
        // is a forge self-attestation of an agent review — exactly the
        // single-party forgery the multi-party scheme exists to prevent
        // (agent-gates.md §What Gets Signed and By Whom). The signature
        // may verify against the forge key; consistency must still fail.
        let mut bundle = sample_bundle();
        bundle.gates[1].signed_by = "forge".to_string();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors.iter().any(|e| e.contains("signed by the forge")),
            "got {errors:?}"
        );
    }

    #[test]
    fn command_gate_signed_by_agent_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.gates[0].signed_by = "agent:security-gate-7".to_string();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("command-run gate results are signed by the forge")),
            "got {errors:?}"
        );
    }

    #[test]
    fn missing_gate_signature_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.gates[0].signature = None;
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors.iter().any(|e| e.contains("carries no signature")),
            "got {errors:?}"
        );
    }

    #[test]
    fn spec_approval_without_spec_ref_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.spec_ref = None;
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("spec_approval without a spec_ref")),
            "got {errors:?}"
        );
    }

    #[test]
    fn malformed_spec_ref_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.spec_ref = Some("specs/system/agent-gates.md".to_string());
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors.iter().any(|e| e.contains("path@sha")),
            "got {errors:?}"
        );
    }

    #[test]
    fn author_oidc_sub_mismatch_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.author.as_mut().unwrap().oidc_sub = "agent:not-worker-42".to_string();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors.iter().any(|e| e.contains("does not match agent id")),
            "got {errors:?}"
        );
    }

    #[test]
    fn wrong_version_fails_consistency() {
        let mut bundle = sample_bundle();
        bundle.merge_attestation_version = "2.0".to_string();
        let errors = check_bundle_consistency(&bundle);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("merge_attestation_version")),
            "got {errors:?}"
        );
    }

    #[test]
    fn assembly_scores_components_and_reports_errors() {
        // All components pass → valid.
        let ok = assemble_verification_result(
            true,
            vec![GateVerificationResult {
                name: "tests".to_string(),
                gate_type: "TestCommand".to_string(),
                signed_by: "forge".to_string(),
                signature_valid: true,
                errors: vec![],
            }],
            None,
            vec![],
        );
        assert!(ok.valid);
        assert!(ok.consistency_check);
        assert!(ok.errors.is_empty());

        // A failed gate signature → invalid, error names the gate.
        let bad_gate = assemble_verification_result(
            true,
            vec![GateVerificationResult {
                name: "tests".to_string(),
                gate_type: "TestCommand".to_string(),
                signed_by: "forge".to_string(),
                signature_valid: false,
                errors: vec![],
            }],
            None,
            vec![],
        );
        assert!(!bad_gate.valid);
        assert!(bad_gate.errors.iter().any(|e| e.contains("tests")));

        // A failed spec approval signature (Some(false)) → invalid; a
        // missing one (None) → still valid.
        assert!(!assemble_verification_result(true, vec![], Some(false), vec![]).valid);
        assert!(assemble_verification_result(true, vec![], None, vec![]).valid);

        // Consistency errors → invalid, consistency_check false.
        let inconsistent = assemble_verification_result(true, vec![], None, vec!["boom".into()]);
        assert!(!inconsistent.valid);
        assert!(!inconsistent.consistency_check);
        assert!(inconsistent.errors.iter().any(|e| e.contains("boom")));
    }
}
