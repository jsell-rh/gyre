//! Pluggable verification of merge attestation bundles
//! (agent-gates.md §Part 3 Verification).
//!
//! Anyone who trusts Gyre's OIDC issuer can verify an entire bundle —
//! offline, after the fact, by auditors, compliance teams, or other
//! Gyre instances (federation). The verification steps that need key
//! material (the bundle signature, each gate's signature against the
//! signer's identity, and the spec approval signature) are behind this
//! port; the structural consistency check (all gate results reference
//! the same MR and spec) is a pure domain function
//! (`gyre_domain::attestation_verification::check_bundle_consistency`).
//!
//! The initial implementation is the server-side
//! `ForgeAttestationVerifier` (Ed25519 keys re-derived from the forge
//! key, as produced by the task-142 signers). The trait is the seam
//! where external/OIDC key resolution plugs in for fully-offline
//! federation verification without changing the bundle format: every
//! implementation verifies the same canonical payload bytes.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::attestation_verification::VerificationResult;
use gyre_domain::merge_attestation::{GateAttestation, MergeAttestationBundle, SpecRefAttestation};

/// A verification party for merge attestation bundles.
///
/// Implementations resolve the key material for each signing party the
/// bundle names ("forge" and "agent:<id>") and verify the canonical
/// payload bytes each signature covers, per the spec's verification
/// steps 1–3. Step 5 (structural consistency) is computed by the pure
/// domain function and combined into the [`VerificationResult`] by
/// `verify_bundle`.
#[async_trait]
#[allow(clippy::double_must_use)] // async-trait 0.1.89 injects #[must_use] into every
                                  // expanded method while the boxed-future return type is
                                  // already #[must_use] (Future); same suppression as
                                  // AttestationSigner and every sibling port.
pub trait AttestationVerifier: Send + Sync {
    /// Run the full verification protocol over `bundle` and return the
    /// per-component outcome: the bundle signature, every gate
    /// signature, the spec approval signature (when one exists to
    /// verify), and the structural consistency check.
    async fn verify_bundle(&self, bundle: &MergeAttestationBundle) -> Result<VerificationResult>;

    /// Verify one gate attestation's signature against the key of the
    /// party its `signed_by` names ("forge" or "agent:<id>"). Returns
    /// `Ok(false)` when the attestation carries no signature or the
    /// signature does not verify — tampered content, a swapped
    /// signature, or a `signed_by` that does not match the signing
    /// party's key all fail here.
    async fn verify_gate_signature(&self, gate: &GateAttestation) -> Result<bool>;

    /// Verify the spec approval's signature. The signature's payload is
    /// the canonical unsigned approval attestation
    /// (`SpecRefAttestation::unsigned_spec_approval_json`) and its
    /// signer is the recorded approver. Fails (Err) when the approval
    /// carries no signature — there is nothing to verify — or when no
    /// verifiable key can be resolved for the approver.
    async fn verify_spec_approval(&self, approval: &SpecRefAttestation) -> Result<bool>;
}
