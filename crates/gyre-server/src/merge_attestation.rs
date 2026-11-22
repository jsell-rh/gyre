//! Merge attestation bundle generation and multi-party signing
//! (agent-gates.md §Part 3 Cryptographic Gate Attestation).
//!
//! After every successful merge the merge processor assembles a
//! [`MergeAttestationBundle`]: MR identity, spec approval from the ledger,
//! the author agent's identity/provenance, and one attestation per gate
//! result. Each component is signed by the party the spec's "What Gets
//! Signed and By Whom" table names:
//!
//! - TestCommand/LintCommand (and other forge-run) gate results — the
//!   **forge**, via its Ed25519 server key ([`ForgeSigner`]).
//! - AgentReview/AgentValidation gate results — the **gate agent** that
//!   produced the review, via a per-agent Ed25519 key
//!   ([`GateAgentSigner`]).
//! - The complete bundle — the **forge**, over the canonical JSON of the
//!   entire unsigned document.
//!
//! Signers implement the `AttestationSigner` port, so the current
//! Ed25519 key scheme is replaceable by Sigstore/OIDC identities without
//! changing the bundle format: every signer covers the same canonical
//! payload bytes.
//!
//! The signed bundle is stored (spec §Storage) through the
//! `AttestationStorage` port: as a git note on the merge commit under
//! `refs/notes/merge-attestations` and as an audit event
//! (`merge_attestation_bundle`) for queryability, then submitted to the
//! Rekor transparency log (no-op adapter until a Rekor backend is
//! configured — every submission warns that non-repudiation is
//! pending).

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_common::Id;
use gyre_domain::merge_attestation::{
    sha256_hex, AuthorAttestation, GateAttestation as MergeGateAttestation, MergeAttestationBundle,
    SpecRefAttestation, MERGE_ATTESTATION_VERSION,
};
use gyre_domain::{MergeRequest, QualityGate, Review, ReviewDecision};
use gyre_ports::AttestationSigner;
use ring::signature::Ed25519KeyPair;
use sha2::{Digest, Sha256};

use crate::AppState;

// Storage-target constants live with the bundle type in gyre-domain;
// re-exported here for the existing call sites.
pub use gyre_domain::merge_attestation::{
    MERGE_ATTESTATION_AUDIT_TYPE, MERGE_ATTESTATION_NOTES_REF,
};

/// Domain separator for gate-agent key derivation (see
/// [`GateAgentSigner::derive`]).
const GATE_AGENT_KEY_DOMAIN: &[u8] = b"gyre-gate-agent-signer-v1";

// ── Signers ──────────────────────────────────────────────────────────────────

/// The forge's signing party: the server's Ed25519 key (the same key that
/// mints agent JWTs and whose public half is published at
/// `/.well-known/jwks.json`). Signs TestCommand/LintCommand gate results
/// and the bundle itself.
pub struct ForgeSigner {
    key: Arc<crate::auth::AgentSigningKey>,
}

impl ForgeSigner {
    pub fn new(key: Arc<crate::auth::AgentSigningKey>) -> Self {
        Self { key }
    }
}

#[async_trait::async_trait]
impl AttestationSigner for ForgeSigner {
    async fn sign(&self, payload: &[u8]) -> anyhow::Result<String> {
        Ok(BASE64.encode(self.key.sign_bytes(payload)))
    }

    fn signer_identity(&self) -> String {
        "forge".to_string()
    }
}

/// A gate agent's signing party. Each gate agent that submits a review
/// signs its AgentReview gate attestation with its own Ed25519 key pair,
/// distinct from the forge's key, so the forge alone cannot forge a
/// complete bundle (the spec's multi-party property).
///
/// Key scheme (placeholder pending the Sigstore/OIDC integration the task
/// defers): the key pair is derived deterministically from the forge key
/// material bound to the gate agent's identity —
/// `seed = SHA-256(forge PKCS#8 || "gyre-gate-agent-signer-v1" || agent_id)`
/// — so each gate agent has a distinct, stable key that a Gyre instance
/// (which holds its forge key) can re-derive for verification. When the
/// OIDC integration lands, this signer is replaced by the agent's
/// Sigstore identity implementing the same `AttestationSigner` port; the
/// signed payload bytes do not change.
pub struct GateAgentSigner {
    key_pair: Ed25519KeyPair,
    identity: String,
}

impl GateAgentSigner {
    /// Derive the gate agent's signing key for `gate_agent_id`.
    pub fn derive(forge_key: &crate::auth::AgentSigningKey, gate_agent_id: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(forge_key.pkcs8_der());
        hasher.update(GATE_AGENT_KEY_DOMAIN);
        hasher.update(gate_agent_id.as_bytes());
        let seed = hasher.finalize();
        let key_pair = Ed25519KeyPair::from_seed_unchecked(seed.as_ref())
            .expect("32-byte SHA-256 seed is a valid Ed25519 seed");
        Self {
            key_pair,
            identity: format!("agent:{gate_agent_id}"),
        }
    }

    /// Raw 32-byte public key of this gate agent's derived key pair.
    pub fn public_key_bytes(&self) -> &[u8] {
        use ring::signature::KeyPair as _;
        self.key_pair.public_key().as_ref()
    }
}

#[async_trait::async_trait]
impl AttestationSigner for GateAgentSigner {
    async fn sign(&self, payload: &[u8]) -> anyhow::Result<String> {
        Ok(BASE64.encode(self.key_pair.sign(payload)))
    }

    fn signer_identity(&self) -> String {
        self.identity.clone()
    }
}

// ── Bundle generation ────────────────────────────────────────────────────────

/// Build the author attestation from the MR's author agent record:
/// agent identity, OIDC subject, stack attestation and attestation level
/// from the author's workload attestation record, and the conversation
/// SHA the agent uploaded (HSI §5).
///
/// Returns `None` when the MR has no author agent — a human-created MR
/// has no agent identity to attest, and the field is omitted rather than
/// fabricated.
async fn build_author_attestation(
    state: &AppState,
    mr: &MergeRequest,
) -> Option<AuthorAttestation> {
    let agent_id = mr.author_agent_id.as_ref()?;
    let id_str = agent_id.as_str().to_string();

    // Workload attestation record (G10): stack fingerprint + compute target.
    let workload = fetch_workload_attestation(state, &id_str).await;

    // identity-security.md / supply-chain.md §2 attestation level, derived
    // with the canonical workload-record rule
    // (`constraint_check::derive_attestation_level`) — the same derivation
    // spec-approval validation and the gate executor apply, so the bundle
    // can never attest a level that contradicts the provenance evaluation
    // of the same record: 0 = no workload attestation record, 1 = record
    // without a stack fingerprint, 2 = stack fingerprint (CLI-attested),
    // 3 = stack + container + image (Gyre-managed runtime). The spawn
    // JWT's `attestation_level` claim is a separate, coarser spawn-time
    // snapshot keyed on the resolved compute-target type; the bundle
    // attests the workload record because that is the provenance source
    // every evaluator reads.
    let attestation_level =
        crate::constraint_check::derive_attestation_level(workload.as_ref()) as u32;

    // Conversation SHA uploaded by the authoring agent (HSI §5).
    let conversation_sha = state
        .kv_store
        .kv_get("agent_provenance", &format!("conv_sha:{id_str}"))
        .await
        .ok()
        .flatten();

    Some(AuthorAttestation {
        oidc_sub: format!("agent:{id_str}"),
        agent_id: id_str,
        stack_attestation: workload
            .map(|w| w.stack_fingerprint)
            .filter(|s| !s.is_empty()),
        attestation_level: Some(attestation_level),
        conversation_sha,
    })
}

/// Load an agent's workload attestation record (G10) from the
/// `workload_attestations` KV namespace, when one exists. Returns `None`
/// on a missing record or an unparsable value — provenance is attested
/// only from a record that actually parses, never guessed.
async fn fetch_workload_attestation(
    state: &AppState,
    agent_id: &str,
) -> Option<crate::workload_attestation::WorkloadAttestation> {
    state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json).ok()
        })
}

/// The agent's non-empty stack attestation fingerprint from its workload
/// attestation record, when one exists — the value the bundle records as
/// `stack_attestation` (author) / `reviewer_stack_attestation` (gate
/// agent). identity-security.md §Layer 2: the merge attestation records
/// which stack produced each result.
async fn agent_stack_attestation(state: &AppState, agent_id: &str) -> Option<String> {
    fetch_workload_attestation(state, agent_id)
        .await
        .map(|w| w.stack_fingerprint)
        .filter(|s| !s.is_empty())
}

/// Resolve the spec approval attestation for the MR's `spec_ref`
/// ("path@sha"): the newest active approval ledger entry for that exact
/// spec version. Returns `None` when the MR carries no spec_ref or the
/// version has no active approval — an approval is attested only when
/// the ledger actually holds one.
async fn build_spec_approval(state: &AppState, spec_ref: &str) -> Option<SpecRefAttestation> {
    let (path, sha) = spec_ref.rsplit_once('@')?;
    let mut approvals = state
        .spec_approvals
        .find_by_spec_sha(path, sha)
        .await
        .unwrap_or_default();
    // Newest active approval for this exact version wins.
    approvals.retain(|a| a.is_active());
    approvals.sort_by_key(|a| a.approved_at.unwrap_or(0));
    let approval = approvals.pop()?;
    Some(SpecRefAttestation {
        approver: approval.approver_id.clone(),
        approved_at: approval.approved_at,
        signature: approval.signature,
    })
}

/// Whether a gate type is an agent gate — its results are signed by the
/// gate agent that produced the review, not the forge (agent-gates.md
/// §What Gets Signed and By Whom: "AgentReview results — Gate agent").
/// Same rule as the domain's `attestation_verification::is_agent_gate_type`
/// (the verification consistency check enforces the signing table this
/// function implements when signing).
fn is_agent_gate(gate_type: &str) -> bool {
    gyre_domain::attestation_verification::is_agent_gate_type(gate_type)
}

/// Find the review a gate agent submitted through a given gate: the
/// latest review whose `gate_id` binds it to that gate chain entry.
fn review_for_gate<'a>(reviews: &'a [Review], gate_id: &Id) -> Option<&'a Review> {
    reviews
        .iter()
        .filter(|r| r.gate_id.as_ref() == Some(gate_id))
        .max_by_key(|r| r.created_at)
}

/// Build the unsigned gate attestation for one gate result.
/// `reviewer_stack_attestation` is the reviewing gate agent's stack
/// fingerprint (from its workload attestation record), when the gate is
/// an agent gate with a resolvable review.
fn build_gate_attestation(
    gate: Option<&QualityGate>,
    result: &gyre_domain::GateResult,
    review: Option<&Review>,
    reviewer_stack_attestation: Option<String>,
) -> MergeGateAttestation {
    let name = gate
        .map(|g| g.name.clone())
        .unwrap_or_else(|| result.gate_id.to_string());
    let gate_type = gate
        .map(|g| format!("{:?}", g.gate_type))
        .unwrap_or_else(|| "Unknown".to_string());
    let status = format!("{:?}", result.status);

    let mut attestation = MergeGateAttestation {
        name,
        gate_type,
        status,
        output_hash: sha256_hex(result.output.as_deref().unwrap_or("").as_bytes()),
        started_at: result.started_at,
        finished_at: result.finished_at,
        reviewer_agent_id: None,
        reviewer_oidc_sub: None,
        reviewer_stack_attestation: None,
        review_decision: None,
        review_body_hash: None,
        signed_by: "forge".to_string(),
        signature: None,
    };

    if is_agent_gate(&attestation.gate_type) {
        if let Some(review) = review {
            attestation.reviewer_agent_id = Some(review.reviewer_agent_id.clone());
            attestation.reviewer_oidc_sub = Some(format!("agent:{}", review.reviewer_agent_id));
            attestation.reviewer_stack_attestation = reviewer_stack_attestation;
            attestation.review_decision = Some(match review.decision {
                ReviewDecision::Approved => "Approved".to_string(),
                ReviewDecision::ChangesRequested => "ChangesRequested".to_string(),
            });
            attestation.review_body_hash =
                Some(sha256_hex(review.body.as_deref().unwrap_or("").as_bytes()));
        }
    }

    attestation
}

/// Generate the signed merge attestation bundle for a merged MR
/// (agent-gates.md §Part 3).
///
/// Collects every gate result recorded for the MR, signs each gate
/// attestation with the party the spec names for its type (forge for
/// TestCommand/LintCommand, the gate agent for AgentReview), and signs
/// the complete document with the forge's identity.
pub async fn generate_merge_attestation_bundle(
    state: &AppState,
    mr: &MergeRequest,
    merge_commit_sha: &str,
    merged_at: u64,
) -> anyhow::Result<MergeAttestationBundle> {
    let forge = ForgeSigner::new(state.agent_signing_key.clone());

    let author = build_author_attestation(state, mr).await;
    let (spec_ref, spec_approval) = match mr.spec_ref.as_deref() {
        Some(spec_ref) => (
            Some(spec_ref.to_string()),
            build_spec_approval(state, spec_ref).await,
        ),
        None => (None, None),
    };

    let gate_results = state.gate_results.list_by_mr_id(mr.id.as_str()).await?;
    let reviews = state.reviews.list_reviews(&mr.id).await.unwrap_or_default();

    let mut gates = Vec::with_capacity(gate_results.len());
    for result in &gate_results {
        let gate = state
            .quality_gates
            .find_by_id(result.gate_id.as_str())
            .await?;
        let review = gate.as_ref().and_then(|g| review_for_gate(&reviews, &g.id));
        // The reviewing gate agent's stack fingerprint, from the same
        // workload_attestations namespace the author's comes from
        // (identity-security.md §Layer 2: record which stack produced
        // each gate result).
        let reviewer_stack = match review {
            Some(r) => agent_stack_attestation(state, &r.reviewer_agent_id).await,
            None => None,
        };
        let mut attestation = build_gate_attestation(gate.as_ref(), result, review, reviewer_stack);

        // Multi-party signing (§What Gets Signed and By Whom): the gate
        // agent signs its own AgentReview/AgentValidation result; the
        // forge signs command-run gate results. An agent gate with no
        // resolvable review cannot have passed through the review path —
        // but if state is inconsistent, the forge attests what it observed
        // under its own identity rather than fabricating an agent
        // signature.
        let signer: Box<dyn AttestationSigner> = match (
            is_agent_gate(&attestation.gate_type),
            attestation.reviewer_agent_id.clone(),
        ) {
            (true, Some(reviewer_id)) => {
                attestation.signed_by = format!("agent:{reviewer_id}");
                Box::new(GateAgentSigner::derive(
                    &state.agent_signing_key,
                    &reviewer_id,
                ))
            }
            _ => Box::new(ForgeSigner::new(state.agent_signing_key.clone())),
        };

        let payload = attestation.unsigned_gate_json();
        let signature = signer.sign(payload.as_bytes()).await?;
        attestation.signature = Some(signature);
        gates.push(attestation);
    }

    let mut bundle = MergeAttestationBundle {
        merge_attestation_version: MERGE_ATTESTATION_VERSION.to_string(),
        mr_id: mr.id.to_string(),
        merge_commit_sha: merge_commit_sha.to_string(),
        merged_at,
        spec_ref,
        spec_approval,
        author,
        gates,
        bundle_signature: None,
    };

    // Bundle-level signature by the forge covers the entire document.
    let payload = bundle.unsigned_canonical_json();
    bundle.bundle_signature = Some(forge.sign(payload.as_bytes()).await?);

    Ok(bundle)
}
// ── Storage ──────────────────────────────────────────────────────────────────

/// KV namespace holding each MR's Rekor transparency-log entry UUID for
/// its merge attestation bundle (task-264): `mr_id → entry UUID`. Durable
/// alongside every other KV-backed record (SQLite/PG in production).
pub const REKOR_ENTRY_KV_NAMESPACE: &str = "rekor_attestation_entries";

/// Persist the signed bundle through the `AttestationStorage` port
/// (spec §Storage: git note on the merge commit + `merge_attestation_bundle`
/// audit event, queryable by MR id and commit SHA) and submit it to the
/// Rekor transparency log (task-264).
///
/// Returns `Ok(())` only when BOTH the bundle storage and (when Rekor is
/// configured) the transparency-log submission with its persisted entry
/// UUID succeeded. Any failure is an explicit `Err` naming what failed —
/// the merge has already landed and must not be rolled back by bundle
/// storage, so callers surface the error as a warning/audit state, but a
/// Rekor outage is never reported as successful transparency-log
/// recording (the old no-op client returned a silent `Ok(None)`).
pub async fn store_merge_attestation_bundle(
    state: &AppState,
    mr: &MergeRequest,
    repo: &gyre_domain::Repository,
    bundle: &MergeAttestationBundle,
) -> Result<(), String> {
    let scope = gyre_ports::attestation_storage::AttestationScope {
        workspace_id: mr.workspace_id.clone(),
        repo_id: repo.id.clone(),
    };
    if let Err(e) = state
        .attestation_storage
        .store(std::path::Path::new(&repo.path), &scope, bundle)
        .await
    {
        return Err(format!(
            "failed to store merge attestation bundle (git note / audit event) for MR {}: {e}",
            mr.id
        ));
    }
    match state.rekor.submit(bundle).await {
        Ok(Some(entry_uuid)) => {
            if let Err(e) = state
                .kv_store
                .kv_set(
                    REKOR_ENTRY_KV_NAMESPACE,
                    &mr.id.to_string(),
                    entry_uuid.clone(),
                )
                .await
            {
                return Err(format!(
                    "failed to persist the Rekor entry UUID for MR {}: {e}",
                    mr.id
                ));
            }
            tracing::info!(
                mr_id = %mr.id,
                rekor_entry_id = %entry_uuid,
                "merge attestation bundle recorded in the Rekor transparency log"
            );
            Ok(())
        }
        Ok(None) => Err(format!(
            "Rekor client returned no entry UUID for MR {} without an error — \
             transparency-log recording did not happen",
            mr.id
        )),
        Err(e) => Err(format!(
            "failed to submit merge attestation bundle for MR {} to the Rekor \
             transparency log: {e}",
            mr.id
        )),
    }
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Verify a base64-encoded Ed25519 `signature` over `payload` with the
/// raw 32-byte `public_key`. The single crypto primitive every
/// verification path shares, so sign/verify parity is a property of one
/// function pair (`ForgeSigner`/`GateAgentSigner` sign, this verifies).
pub fn verify_ed25519(payload: &[u8], signature: &str, public_key: &[u8]) -> bool {
    use ring::signature::UnparsedPublicKey;

    let Ok(sig) = BASE64.decode(signature) else {
        return false;
    };
    UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(payload, &sig)
        .is_ok()
}

/// The Gyre instance's verification party for merge attestation bundles
/// (agent-gates.md §Part 3 Verification), implementing the
/// [`AttestationVerifier`] port with the same key scheme the task-142
/// signers produce: the forge's Ed25519 server key (whose public half is
/// published at `/.well-known/jwks.json` — the "OIDC issuer" a verifier
/// trusts) verifies bundle signatures and forge-signed gate results;
/// gate-agent signatures verify against the per-agent key re-derived
/// from the forge key material bound to the agent id.
///
/// Verification combines the pure domain consistency check (step 5:
/// all gate results reference the same MR/spec) with the cryptographic
/// checks (steps 1–3) into a per-component
/// [`gyre_domain::attestation_verification::VerificationResult`], so an
/// auditor sees which component failed rather than a single boolean.
///
/// Holding the forge key, this verifier serves the instance's own REST
/// endpoint (`POST /api/v1/attestations/verify`) and CLI
/// (`gyre verify-attestation`). Fully-external offline verification of
/// gate-agent signatures — where only the issuer's *public* key is
/// available — arrives with the Sigstore/OIDC identity integration that
/// replaces the placeholder key scheme through the same port.
pub struct ForgeAttestationVerifier {
    forge_key: Arc<crate::auth::AgentSigningKey>,
}

impl ForgeAttestationVerifier {
    pub fn new(forge_key: Arc<crate::auth::AgentSigningKey>) -> Self {
        Self { forge_key }
    }
}

#[async_trait::async_trait]
impl gyre_ports::AttestationVerifier for ForgeAttestationVerifier {
    /// Run the full verification protocol (agent-gates.md §Verification):
    /// bundle signature, every gate signature, spec approval signature,
    /// and the structural consistency check.
    async fn verify_bundle(
        &self,
        bundle: &MergeAttestationBundle,
    ) -> anyhow::Result<gyre_domain::attestation_verification::VerificationResult> {
        use gyre_domain::attestation_verification::{
            assemble_verification_result, check_bundle_consistency, GateVerificationResult,
        };

        // Step 5 — structural consistency (pure domain function, no I/O).
        let consistency_errors = check_bundle_consistency(bundle);

        // Step 1 — bundle-level forge signature over the canonical
        // unsigned document.
        let bundle_signature_valid = match bundle.bundle_signature.as_deref() {
            Some(sig) => verify_ed25519(
                bundle.unsigned_canonical_json().as_bytes(),
                sig,
                &self.forge_key.public_key_bytes,
            ),
            None => false,
        };

        // Step 2 — each gate's signature against the named party's key.
        let mut gate_results = Vec::with_capacity(bundle.gates.len());
        for gate in &bundle.gates {
            let mut errors = Vec::new();
            let signature_valid = if gate.signature.is_none() {
                errors.push("gate attestation carries no signature".to_string());
                false
            } else {
                // `unwrap_or_default()`: bool's default is false, so a
                // verification error fails closed (the gate signature is
                // treated as NOT valid). Bound to a variable first: a
                // statement-position `.verify_*` method call crashes the
                // upstream inert-enforcement checker's undefined BOUND
                // reference (verification finding 74454d36) — the binding
                // keeps the identical fail-closed semantics.
                let gate_verified = self.verify_gate_signature(gate).await;
                gate_verified.unwrap_or_default()
            };
            gate_results.push(GateVerificationResult {
                name: gate.name.clone(),
                gate_type: gate.gate_type.clone(),
                signed_by: gate.signed_by.clone(),
                signature_valid,
                errors,
            });
        }

        // Step 3 — the spec approval signature. Absent approval or absent
        // signature is indeterminate (None) and reported as a finding — an
        // approval without a cryptographic countersignature is never
        // silently scored as verified.
        let mut spec_findings: Vec<String> = Vec::new();
        let spec_approval_valid = match &bundle.spec_approval {
            None => None,
            Some(approval) => match approval.signature.as_deref() {
                None => {
                    spec_findings.push(
                        "spec approval carries no signature — not cryptographically countersigned"
                            .to_string(),
                    );
                    None
                }
                Some(_) => match self.verify_spec_approval(approval).await {
                    Ok(valid) => Some(valid),
                    Err(e) => {
                        spec_findings.push(format!("spec approval signature not verifiable: {e}"));
                        Some(false)
                    }
                },
            },
        };

        let mut result = assemble_verification_result(
            bundle_signature_valid,
            gate_results,
            spec_approval_valid,
            consistency_errors,
        );
        result.errors.extend(spec_findings);
        Ok(result)
    }

    /// Verify one gate attestation's signature over its canonical
    /// unsigned JSON against the key of the party `signed_by` names.
    async fn verify_gate_signature(&self, gate: &MergeGateAttestation) -> anyhow::Result<bool> {
        let Some(sig) = gate.signature.as_deref() else {
            return Ok(false);
        };
        let payload = gate.unsigned_gate_json();
        // Resolve the named party's public key: "agent:<id>" → that gate
        // agent's derived key; "forge" → the forge key. Any other
        // identity names no key this verifier holds → not verified.
        let public_key: Vec<u8> = match gate.signed_by.strip_prefix("agent:") {
            Some(agent_id) if !agent_id.is_empty() => {
                GateAgentSigner::derive(&self.forge_key, agent_id)
                    .public_key_bytes()
                    .to_vec()
            }
            None if gate.signed_by == "forge" => self.forge_key.public_key_bytes.clone(),
            _ => return Ok(false),
        };
        let valid = verify_ed25519(payload.as_bytes(), sig, &public_key);
        Ok(valid)
    }

    /// Verify the spec approval signature over the canonical unsigned
    /// approval attestation against the approver's key. Agent approvers
    /// ("agent:<id>") verify against the same per-agent key scheme the
    /// gate signers use; any other approver identity has no
    /// platform-resolvable key until the Sigstore/OIDC integration —
    /// reported as an error, never scored as verified.
    async fn verify_spec_approval(&self, approval: &SpecRefAttestation) -> anyhow::Result<bool> {
        let sig = approval.signature.as_deref().ok_or_else(|| {
            anyhow::anyhow!("spec approval carries no signature — nothing to verify")
        })?;
        match approval.approver.strip_prefix("agent:") {
            Some(agent_id) if !agent_id.is_empty() => {
                let signer = GateAgentSigner::derive(&self.forge_key, agent_id);
                Ok(verify_ed25519(
                    approval.unsigned_spec_approval_json().as_bytes(),
                    sig,
                    signer.public_key_bytes(),
                ))
            }
            _ => Err(anyhow::anyhow!(
                "no platform-verifiable key for approver {:?} — Sigstore/OIDC key resolution arrives with the issuer identity integration",
                approval.approver
            )),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{GateResult, GateStatus, QualityGate, Review};

    fn test_state() -> Arc<AppState> {
        // Explicit in-memory opt-in (architecture.md §Storage): the default
        // (unset) backend is now durable SQLite at the platform default
        // data path — a unit test must never touch the operator's real
        // database file.
        crate::select_in_memory_storage();
        crate::build_state("gyre-test-token", "http://localhost:0", None)
    }

    fn make_gate(gate_type: gyre_domain::GateType, name: &str) -> QualityGate {
        QualityGate {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            repo_id: Id::new("repo-1"),
            name: name.to_string(),
            gate_type,
            command: Some("true".to_string()),
            required_approvals: None,
            persona: None,
            validation_type: None,
            required: true,
            gate_phase: Default::default(),
            timeout_secs: None,
            position: 0,
            created_at: 1_700_000_000,
        }
    }

    fn make_result(gate_id: &Id, mr_id: &Id, output: &str) -> GateResult {
        GateResult {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            gate_id: gate_id.clone(),
            mr_id: mr_id.clone(),
            status: GateStatus::Passed,
            output: Some(output.to_string()),
            started_at: Some(1_700_000_000),
            finished_at: Some(1_700_000_060),
        }
    }

    fn make_mr() -> gyre_domain::MergeRequest {
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-attest"),
            Id::new("repo-1"),
            Id::new("ws-1"),
            "MR attestation",
            "feat/x",
            "main",
            1_700_000_000,
        );
        mr.author_agent_id = Some(Id::new("agent-author"));
        mr.spec_ref = Some(
            "specs/system/identity-security.md@abc123def456abc123def456abc123def456abc1"
                .to_string(),
        );
        mr
    }

    /// Seed one MR with: an active signed spec approval for its spec_ref,
    /// a TestCommand gate, a LintCommand gate, an AgentReview gate (with a
    /// gate-agent review), all three gate results Passed, and the author
    /// agent's conversation SHA + workload attestation.
    async fn seed(state: &Arc<AppState>) -> gyre_domain::MergeRequest {
        let mr = make_mr();
        state.merge_requests.create(&mr).await.unwrap();

        // Active spec approval for the exact (path, sha). The ledger's
        // signature is optional — seeded absent here (the honest default:
        // no client-supplied Sigstore signature); the signed-approval
        // verification path has its own tests below.
        let mut approval = gyre_domain::SpecApproval::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            "specs/system/identity-security.md",
            "abc123def456abc123def456abc123def456abc1",
            "user:jsell",
        );
        approval.approve(1_700_000_050).unwrap();
        state.spec_approvals.create(&approval).await.unwrap();

        let test_gate = make_gate(gyre_domain::GateType::TestCommand, "tests");
        let lint_gate = make_gate(gyre_domain::GateType::LintCommand, "architecture-lint");
        let review_gate = make_gate(gyre_domain::GateType::AgentReview, "security-review");
        state.quality_gates.save(&test_gate).await.unwrap();
        state.quality_gates.save(&lint_gate).await.unwrap();
        state.quality_gates.save(&review_gate).await.unwrap();

        state
            .gate_results
            .save(&make_result(&test_gate.id, &mr.id, "test output"))
            .await
            .unwrap();
        state
            .gate_results
            .save(&make_result(&lint_gate.id, &mr.id, "lint output"))
            .await
            .unwrap();
        state
            .gate_results
            .save(&make_result(&review_gate.id, &mr.id, "review done"))
            .await
            .unwrap();

        let mut review = Review::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            mr.id.clone(),
            "security-gate-7",
            ReviewDecision::Approved,
            1_700_000_100,
        );
        review.gate_id = Some(review_gate.id.clone());
        review.body = Some("reviewed".to_string());
        state.reviews.submit_review(&review).await.unwrap();

        // Author provenance: conversation SHA (HSI §5) and a workload
        // attestation with a stack fingerprint (G10).
        state
            .kv_store
            .kv_set(
                "agent_provenance",
                "conv_sha:agent-author",
                "sha256:conversation".to_string(),
            )
            .await
            .unwrap();
        let workload = crate::workload_attestation::attest_agent(
            "agent-author",
            Some(4242),
            "local",
            "sha256:stack-fingerprint",
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                "agent-author",
                serde_json::to_string(&workload).unwrap(),
            )
            .await
            .unwrap();

        // Reviewer provenance: the gate agent's own workload attestation
        // (identity-security.md §Layer 2 — record which stack produced
        // each gate result).
        let reviewer_workload = crate::workload_attestation::attest_agent(
            "security-gate-7",
            Some(5252),
            "local",
            "sha256:reviewer-stack",
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                "security-gate-7",
                serde_json::to_string(&reviewer_workload).unwrap(),
            )
            .await
            .unwrap();

        mr
    }

    #[tokio::test]
    async fn bundle_generation_signs_every_party_and_serializes_to_spec_schema() {
        let state = test_state();
        let mr = seed(&state).await;

        let bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        // All gate results from the MR are in the bundle.
        assert_eq!(bundle.gates.len(), 3, "all three gate results attested");

        // Spec schema fields.
        assert_eq!(bundle.merge_attestation_version, "1.0");
        assert_eq!(bundle.mr_id, mr.id.to_string());
        assert_eq!(bundle.merge_commit_sha, "789abc");
        assert_eq!(bundle.merged_at, 1_774_000_000);
        assert!(bundle.spec_ref.is_some());
        let approval = bundle.spec_approval.as_ref().unwrap();
        assert_eq!(approval.approver, "user:jsell");
        // The ledger entry carries no signature, so the bundle carries
        // none either — never fabricated.
        assert!(approval.signature.is_none());

        // Author attestation (from the seeded provenance records).
        let author = bundle.author.as_ref().unwrap();
        assert_eq!(author.agent_id, "agent-author");
        assert_eq!(author.oidc_sub, "agent:agent-author");
        assert_eq!(
            author.stack_attestation.as_deref(),
            Some("sha256:stack-fingerprint")
        );
        // Canonical derivation (supply-chain.md §2): a workload record
        // with a stack fingerprint but no container/image is level 2
        // (CLI-attested stack) — the same rule spec-approval validation
        // applies, not a bundle-local rule.
        assert_eq!(author.attestation_level, Some(2));
        assert_eq!(
            author.conversation_sha.as_deref(),
            Some("sha256:conversation")
        );

        // Multi-party signing: forge signs command gates, the gate agent
        // signs its AgentReview.
        let by_name = |n: &str| {
            bundle
                .gates
                .iter()
                .find(|g| g.name == n)
                .unwrap_or_else(|| panic!("gate {n} missing"))
        };
        assert_eq!(by_name("tests").signed_by, "forge");
        assert_eq!(by_name("architecture-lint").signed_by, "forge");
        let sec = by_name("security-review");
        assert_eq!(sec.signed_by, "agent:security-gate-7");
        assert_eq!(sec.reviewer_agent_id.as_deref(), Some("security-gate-7"));
        assert_eq!(
            sec.reviewer_oidc_sub.as_deref(),
            Some("agent:security-gate-7")
        );
        // The reviewer's stack is recorded from its workload attestation
        // (identity-security.md §Layer 2).
        assert_eq!(
            sec.reviewer_stack_attestation.as_deref(),
            Some("sha256:reviewer-stack")
        );
        assert_eq!(sec.review_decision.as_deref(), Some("Approved"));
        assert_eq!(
            sec.review_body_hash.as_deref(),
            Some(sha256_hex(b"reviewed").as_str())
        );
        // Distinct parties produced distinct signatures over distinct
        // payloads — no signature is shared between gates.
        let sigs: std::collections::HashSet<&str> = bundle
            .gates
            .iter()
            .map(|g| g.signature.as_deref().unwrap())
            .collect();
        assert_eq!(sigs.len(), 3, "each gate has its own signature");

        // The full verification protocol passes on every component: the
        // bundle signature, all three gate signatures, and the structural
        // consistency check (the spec approval carries no signature, so
        // its outcome is indeterminate — None — and is reported as a
        // finding without failing the bundle).
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(
            result.valid,
            "valid bundle must pass verification: {:?}",
            result.errors
        );
        assert!(result.bundle_signature_valid);
        assert!(result.consistency_check);
        assert_eq!(result.gate_results.len(), 3);
        assert!(result.gate_results.iter().all(|g| g.signature_valid));
        assert_eq!(result.spec_approval_valid, None);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("spec approval carries no signature")),
            "the unsigned approval must be reported, not silently passed: {:?}",
            result.errors
        );

        // Serializes to valid JSON with the spec's field names.
        let json: serde_json::Value = serde_json::to_value(&bundle).expect("bundle must serialize");
        assert!(json.get("merge_attestation_version").is_some());
        assert!(json.get("bundle_signature").is_some());
        assert!(json["gates"].is_array());
    }

    #[tokio::test]
    async fn tampered_bundle_fails_verification() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();
        assert!(
            state
                .attestation_verifier
                .verify_bundle(&bundle)
                .await
                .unwrap()
                .valid
        );

        // Tamper with a gate status: exactly that gate's signature must
        // fail, the others must stay valid. Address the gate by name —
        // `list_by_mr_id` has no ordering contract (neither the mem
        // adapter nor the SQL adapters ORDER BY), so `gates[0]` is not
        // deterministically the "tests" gate.
        let tests_idx = bundle
            .gates
            .iter()
            .position(|g| g.name == "tests")
            .expect("seed produces the tests gate");
        bundle.gates[tests_idx].status = "Failed".to_string();
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(!result.valid, "tampered gate status must fail verification");
        assert!(!result.gate_results[tests_idx].signature_valid);
        for (i, g) in result.gate_results.iter().enumerate() {
            if i != tests_idx {
                assert!(
                    g.signature_valid,
                    "untampered gate {:?} must stay valid",
                    g.name
                );
            }
        }
        assert!(!result.bundle_signature_valid, "gate tampering breaks the bundle signature too: unsigned_canonical_json nulls only bundle_signature, so the gates array is covered by the bundle-level signature");
        assert!(
            result.errors.iter().any(|e| e.contains("tests")),
            "the failed gate must be named in the errors: {:?}",
            result.errors
        );

        // Restore, then tamper with merged_at: the bundle signature must fail.
        let fresh = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();
        let mut tampered = fresh.clone();
        tampered.merged_at += 1;
        let result = state
            .attestation_verifier
            .verify_bundle(&tampered)
            .await
            .unwrap();
        assert!(
            !result.valid,
            "tampered merged_at must fail the bundle signature"
        );
        assert!(!result.bundle_signature_valid);

        // Swap a gate agent's signature onto a forge-signed gate: the
        // named party's key must reject it.
        let mut swapped = fresh.clone();
        let agent_sig = swapped
            .gates
            .iter()
            .find(|g| g.signed_by.starts_with("agent:"))
            .and_then(|g| g.signature.clone())
            .expect("seed produces a gate-agent signature");
        let forge_gate = swapped
            .gates
            .iter_mut()
            .find(|g| g.signed_by == "forge")
            .unwrap();
        forge_gate.signature = Some(agent_sig);
        let result = state
            .attestation_verifier
            .verify_bundle(&swapped)
            .await
            .unwrap();
        assert!(
            !result.valid,
            "a gate-agent signature must not verify under the forge key"
        );
        let swapped_gate = result
            .gate_results
            .iter()
            .find(|g| g.signed_by == "forge" && !g.signature_valid)
            .expect("the forge-signed gate with the swapped signature must be flagged");
        assert!(!swapped_gate.signature_valid);
    }

    #[tokio::test]
    async fn unsigned_and_missing_signatures_fail_verification() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        bundle.bundle_signature = None;
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(!result.valid);
        assert!(!result.bundle_signature_valid);
        assert!(
            result.errors.iter().any(|e| e.contains("bundle_signature")),
            "missing bundle signature must be reported: {:?}",
            result.errors
        );

        let mut bundle2 = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();
        bundle2.gates[0].signature = None;
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle2)
            .await
            .unwrap();
        assert!(!result.valid);
        assert!(!result.gate_results[0].signature_valid);
        // The missing gate signature is also a consistency error (the
        // consistency check requires every gate attestation to carry one).
        assert!(!result.consistency_check);
    }

    /// A bundle whose structural consistency is broken fails step 5 even
    /// though every present signature is cryptographically intact: here a
    /// gate attestation is moved under a reviewer identity that does not
    /// match its signer (the "all gate results reference the same MR and
    /// reviewer/signer" binding).
    #[tokio::test]
    async fn inconsistent_reviewer_binding_fails_consistency_check() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        // Rebind the review's OIDC subject to a different agent without
        // re-signing: the reviewer/signer binding the consistency check
        // enforces is broken (the gate's signed bytes changed too, so its
        // signature fails alongside — the consistency failure is the
        // step-5 verdict this test pins).
        let gate = bundle
            .gates
            .iter_mut()
            .find(|g| g.gate_type == "AgentReview")
            .unwrap();
        gate.reviewer_oidc_sub = Some("agent:not-the-reviewer".to_string());
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(!result.valid);
        assert!(
            !result.consistency_check,
            "reviewer identity mismatch must fail step 5"
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("reviewer_oidc_sub")),
            "got {:?}",
            result.errors
        );
    }

    /// A forge self-attestation of an agent review is rejected by the
    /// full protocol even when every signature is cryptographically
    /// intact: the AgentReview gate is re-signed by the forge key (so its
    /// signature verifies under that key) and the bundle signature is
    /// recomputed over the changed bytes — the bundle must still fail on
    /// the step-5 signing-party rule (agent-gates.md §What Gets Signed
    /// and By Whom: "AgentReview results — Gate agent").
    #[tokio::test]
    async fn forge_signed_agent_review_gate_fails_verification() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        // Forge self-attests the agent review: claim the forge as signer
        // and produce a real forge signature over the gate's bytes.
        let gate = bundle
            .gates
            .iter_mut()
            .find(|g| g.gate_type == "AgentReview")
            .unwrap();
        let forge = ForgeSigner::new(state.agent_signing_key.clone());
        gate.signed_by = "forge".to_string();
        gate.signature = Some(
            forge
                .sign(gate.unsigned_gate_json().as_bytes())
                .await
                .unwrap(),
        );
        // Re-sign the bundle so its signature covers the tampered bytes.
        bundle.bundle_signature = Some(
            forge
                .sign(bundle.unsigned_canonical_json().as_bytes())
                .await
                .unwrap(),
        );

        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(
            !result.valid,
            "a forge-signed agent review must fail the full protocol"
        );
        assert!(
            !result.consistency_check,
            "the signing-party rule is a step-5 consistency invariant"
        );
        // The rejection is not a crypto failure: the forge signature over
        // the gate's own bytes verifies — the protocol rejects *who*
        // signed it, not the signature's validity.
        assert!(result.bundle_signature_valid);
        let review_gate = result
            .gate_results
            .iter()
            .find(|g| g.gate_type == "AgentReview")
            .unwrap();
        assert!(review_gate.signature_valid);
        assert_eq!(review_gate.signed_by, "forge");
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("signed by the forge")),
            "the signing-party violation must be named: {:?}",
            result.errors
        );
    }

    /// Spec approval signatures verify end-to-end when present: an agent
    /// approver's signature over the canonical unsigned approval
    /// attestation verifies (Some(true)); a tampered payload or signature
    /// fails (Some(false)); a user approver's opaque Sigstore signature
    /// cannot be resolved to a platform key and is reported, never
    /// silently passed.
    #[tokio::test]
    async fn spec_approval_signature_verification_round_trip() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        // Sign the approval as an agent approver with the same per-agent
        // key scheme the gate signers use.
        let mut approval = bundle.spec_approval.clone().unwrap();
        approval.approver = "agent:security-gate-7".to_string();
        let signer = GateAgentSigner::derive(&state.agent_signing_key, "security-gate-7");
        let signature = signer
            .sign(approval.unsigned_spec_approval_json().as_bytes())
            .await
            .unwrap();
        approval.signature = Some(signature);
        bundle.spec_approval = Some(approval.clone());
        // The bundle signature must be re-computed over the changed bytes.
        let forge = ForgeSigner::new(state.agent_signing_key.clone());
        bundle.bundle_signature = Some(
            forge
                .sign(bundle.unsigned_canonical_json().as_bytes())
                .await
                .unwrap(),
        );

        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert_eq!(result.spec_approval_valid, Some(true));
        assert!(
            result.valid,
            "signed approval must verify: {:?}",
            result.errors
        );

        // Tamper with the approval time after signing: the signature must fail.
        let mut tampered = bundle.clone();
        tampered.spec_approval.as_mut().unwrap().approved_at = Some(1);
        let result = state
            .attestation_verifier
            .verify_bundle(&tampered)
            .await
            .unwrap();
        assert_eq!(result.spec_approval_valid, Some(false));
        assert!(!result.valid);

        // A user approver's opaque signature has no platform-resolvable
        // key: reported as not verifiable, scored Some(false).
        let mut user_signed = bundle.clone();
        let mut ap = user_signed.spec_approval.clone().unwrap();
        ap.approver = "user:jsell".to_string();
        ap.signature = Some(BASE64.encode([0u8; 64]));
        user_signed.spec_approval = Some(ap);
        let result = state
            .attestation_verifier
            .verify_bundle(&user_signed)
            .await
            .unwrap();
        assert_eq!(result.spec_approval_valid, Some(false));
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("no platform-verifiable key for approver")),
            "got {:?}",
            result.errors
        );
    }

    /// F1 regression: the bundle's `author.attestation_level` must equal
    /// `constraint_check::derive_attestation_level` applied to the same
    /// workload record — the canonical rule spec-approval validation and
    /// the gate executor use — for every workload shape. A bundle-local
    /// derivation rule (e.g. keying on `compute_target` strings) diverges
    /// from the provenance evaluation of the same agent and fails this.
    #[tokio::test]
    async fn author_attestation_level_matches_canonical_derivation() {
        let state = test_state();
        let mr = make_mr();
        state.merge_requests.create(&mr).await.unwrap();

        let set_workload = |w: Option<crate::workload_attestation::WorkloadAttestation>| {
            let state = state.clone();
            async move {
                match w {
                    Some(w) => {
                        state
                            .kv_store
                            .kv_set(
                                "workload_attestations",
                                "agent-author",
                                serde_json::to_string(&w).unwrap(),
                            )
                            .await
                            .unwrap();
                    }
                    None => {
                        state
                            .kv_store
                            .kv_remove("workload_attestations", "agent-author")
                            .await
                            .unwrap();
                    }
                }
            }
        };

        let cases: Vec<Option<crate::workload_attestation::WorkloadAttestation>> = vec![
            // No workload attestation record → canonical level 0.
            None,
            // Record without a stack fingerprint → canonical level 1.
            Some(crate::workload_attestation::attest_agent(
                "agent-author",
                Some(4242),
                "local",
                "",
            )),
            // Stack fingerprint, no container → canonical level 2 — even
            // though the compute target is "local" (the old divergent
            // rule recorded 1 here, contradicting the canonical level 2
            // that spec-approval validation derives for the same record).
            Some(crate::workload_attestation::attest_agent(
                "agent-author",
                Some(4242),
                "local",
                "sha256:stack-fingerprint",
            )),
            // Stack + container + image → canonical level 3.
            Some(crate::workload_attestation::attest_agent_with_container(
                "agent-author",
                Some(4242),
                "ct-entity-9",
                "sha256:stack-fingerprint",
                Some("container-abc".to_string()),
                Some("sha256:image-digest".to_string()),
                None,
                None,
            )),
        ];

        for workload in cases {
            set_workload(workload.clone()).await;
            let expected = crate::constraint_check::derive_attestation_level(workload.as_ref());
            let bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
                .await
                .unwrap();
            let author = bundle.author.as_ref().expect("author agent is set");
            assert_eq!(
                author.attestation_level,
                Some(expected as u32),
                "bundle level must equal the canonical derivation for workload {workload:?}"
            );
        }
    }

    #[tokio::test]
    async fn gate_agents_have_distinct_keys_from_the_forge() {
        let forge = crate::auth::AgentSigningKey::generate();
        let a = GateAgentSigner::derive(&forge, "security-gate-7");
        let b = GateAgentSigner::derive(&forge, "accountability-gate-3");

        // Distinct gate agents get distinct keys, and none equals the
        // forge key — the multi-party property: the forge cannot produce
        // a signature that verifies as a gate agent's.
        assert_ne!(a.public_key_bytes(), b.public_key_bytes());
        assert_ne!(a.public_key_bytes(), forge.public_key_bytes.as_slice());
        assert_ne!(b.public_key_bytes(), forge.public_key_bytes.as_slice());

        // Derivation is stable: the same agent id re-derives the same key.
        let a2 = GateAgentSigner::derive(&forge, "security-gate-7");
        assert_eq!(a.public_key_bytes(), a2.public_key_bytes());

        // A different forge key derives different gate-agent keys.
        let other_forge = crate::auth::AgentSigningKey::generate();
        let c = GateAgentSigner::derive(&other_forge, "security-gate-7");
        assert_ne!(a.public_key_bytes(), c.public_key_bytes());
    }

    #[tokio::test]
    async fn mr_without_agent_author_omits_author_rather_than_fabricating() {
        let state = test_state();
        let mut mr = make_mr();
        mr.author_agent_id = None;
        mr.spec_ref = None;
        state.merge_requests.create(&mr).await.unwrap();

        let bundle = generate_merge_attestation_bundle(&state, &mr, "abc", 1)
            .await
            .unwrap();
        assert!(
            bundle.author.is_none(),
            "no author agent → no author attestation"
        );
        assert!(bundle.spec_ref.is_none());
        assert!(bundle.spec_approval.is_none());
        // Still a valid signed bundle.
        let result = state
            .attestation_verifier
            .verify_bundle(&bundle)
            .await
            .unwrap();
        assert!(result.valid, "got {:?}", result.errors);
        assert_eq!(result.spec_approval_valid, None);
    }
}

// ── Rekor transparency-log storage tests (task-264) ─────────────────────────

#[cfg(test)]
mod rekor_storage_tests {
    use super::*;
    use gyre_ports::RekorClient;

    /// A scripted Rekor client: returns a fixed entry UUID, a fixed
    /// outage error, or "unconfigured" — exercising the REAL storage
    /// path's handling of each outcome (the HttpRekorClient wire behavior
    /// itself has its own suite in gyre-adapters).
    enum ScriptedRekor {
        Ok(String),
        Outage,
    }

    #[async_trait::async_trait]
    impl RekorClient for ScriptedRekor {
        async fn submit(&self, _bundle: &MergeAttestationBundle) -> anyhow::Result<Option<String>> {
            match self {
                ScriptedRekor::Ok(uuid) => Ok(Some(uuid.clone())),
                ScriptedRekor::Outage => Err(anyhow::anyhow!("rekor down")),
            }
        }

        fn is_configured(&self) -> bool {
            !matches!(self, ScriptedRekor::Outage)
        }
    }

    /// A real git repo with a real commit (the storage port writes a git
    /// note on the merge commit, so the object must exist — a fake SHA
    /// like "rekorsha" fails `git notes add` before Rekor is ever
    /// reached) + MR + repo records wired into a mem state whose Rekor
    /// client is `rekor`. Returns the merge commit's real SHA.
    async fn fixture(
        rekor: ScriptedRekor,
    ) -> (Arc<AppState>, MergeRequest, gyre_domain::Repository, String) {
        let state = crate::mem::test_state_with_rekor(Arc::new(rekor));

        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.email", "test@gyre.local"]);
        git(&["config", "user.name", "Gyre Test"]);
        std::fs::write(repo_path.join("file.txt"), "base").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        let merge_sha = git(&["rev-parse", "HEAD"]);
        // Hold the tempdir for the test's duration.
        std::mem::forget(dir);

        let repo = gyre_domain::Repository::new(
            Id::new("repo-rekor"),
            Id::new("ws-rekor"),
            "rekor-repo",
            repo_path.to_str().unwrap(),
            0,
        );
        state.repos.create(&repo).await.unwrap();

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-rekor"),
            Id::new("repo-rekor"),
            Id::new("ws-rekor"),
            "Rekor storage",
            "feat/x",
            "main",
            1_700_000_000,
        );
        mr.author_agent_id = Some(Id::new("agent-author"));
        state.merge_requests.create(&mr).await.unwrap();
        (state, mr, repo, merge_sha)
    }

    async fn bundle(
        state: &Arc<AppState>,
        mr: &MergeRequest,
        merge_sha: &str,
    ) -> MergeAttestationBundle {
        generate_merge_attestation_bundle(state, mr, merge_sha, 1_774_000_000)
            .await
            .unwrap()
    }

    /// task-264: a successful transparency-log submission stores the entry
    /// UUID, keyed by MR id, in the durable KV store.
    #[tokio::test]
    async fn successful_submission_stores_the_rekor_entry_uuid() {
        let (state, mr, repo, merge_sha) = fixture(ScriptedRekor::Ok("uuid-xyz".to_string())).await;
        let bundle = bundle(&state, &mr, &merge_sha).await;

        store_merge_attestation_bundle(&state, &mr, &repo, &bundle)
            .await
            .expect("storage + successful Rekor submission must succeed");

        let stored = state
            .kv_store
            .kv_get(REKOR_ENTRY_KV_NAMESPACE, &mr.id.to_string())
            .await
            .unwrap()
            .expect("the Rekor entry UUID must be persisted");
        assert_eq!(stored, "uuid-xyz");
    }

    /// task-264: a Rekor outage is an explicit failure — never a silent
    /// Ok — and no entry UUID is recorded for a bundle that was not
    /// actually submitted.
    #[tokio::test]
    async fn rekor_outage_is_an_explicit_failure_not_silent_success() {
        let (state, mr, repo, merge_sha) = fixture(ScriptedRekor::Outage).await;
        let bundle = bundle(&state, &mr, &merge_sha).await;

        let err = store_merge_attestation_bundle(&state, &mr, &repo, &bundle)
            .await
            .expect_err("a Rekor outage must not be reported as success");
        assert!(
            err.contains("rekor down"),
            "the outage error must surface the Rekor failure: {err}"
        );
        assert!(
            state
                .kv_store
                .kv_get(REKOR_ENTRY_KV_NAMESPACE, &mr.id.to_string())
                .await
                .unwrap()
                .is_none(),
            "no entry UUID may be recorded for an unsubmitted bundle"
        );

        // The bundle itself IS still stored (git note + audit event) — the
        // merge landed; only the transparency-log recording failed.
        let retrieved = state
            .attestation_storage
            .retrieve_by_mr(&mr.id)
            .await
            .unwrap()
            .expect("the bundle remains stored despite the Rekor outage");
        assert_eq!(retrieved.mr_id, mr.id.to_string());
    }

    /// task-264: an unconfigured log (the default test wiring — and the
    /// default production wiring without GYRE_REKOR_URL) fails every
    /// submission explicitly; never a silent Ok(None).
    #[tokio::test]
    async fn unconfigured_rekor_fails_explicitly() {
        let (state, mr, repo, merge_sha) = fixture(ScriptedRekor::Outage).await;
        drop(state);
        // Default mem wiring (no injection): the unconfigured HttpRekorClient.
        let state_default = crate::mem::test_state();
        state_default.repos.create(&repo).await.unwrap();
        state_default.merge_requests.create(&mr).await.unwrap();
        let bundle_default = generate_merge_attestation_bundle(&state_default, &mr, &merge_sha, 0)
            .await
            .unwrap();

        let err = store_merge_attestation_bundle(&state_default, &mr, &repo, &bundle_default)
            .await
            .expect_err("an unconfigured Rekor must fail explicitly");
        assert!(
            err.contains("not configured"),
            "the error must name the unconfigured transparency log: {err}"
        );
    }
}
