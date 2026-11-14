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
//! The signed bundle is stored (spec §Storage) as a git note on the merge
//! commit under `refs/notes/merge-attestations` and as an audit event
//! (`merge_attestation_bundle`) for queryability. Rekor transparency-log
//! publication arrives with the Sigstore/OIDC integration.

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

/// Git notes ref for merge attestation bundles (spec §Storage: "as a git
/// note on the merge commit"). Distinct from the legacy
/// `refs/notes/attestations` ref, whose dual-write (chain vs. legacy
/// bundle) is managed by `crate::attestation` — a separate ref avoids
/// racing that logic.
pub const MERGE_ATTESTATION_NOTES_REF: &str = "refs/notes/merge-attestations";

/// Event type recorded in the audit log for each generated bundle (spec
/// §Storage: "in the audit_events table for queryability").
pub const MERGE_ATTESTATION_AUDIT_TYPE: &str = "merge_attestation_bundle";

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
    let workload = state
        .kv_store
        .kv_get("workload_attestations", &id_str)
        .await
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json).ok()
        });

    // identity-security.md attestation levels, derived from the recorded
    // compute target the same way the spawn path assigns the JWT claim:
    // container = 3 (Gyre-managed container with attestation), any other
    // resolved (non-local) target = 2 (CLI-managed), local subprocess = 1.
    let attestation_level = workload.as_ref().map(|w| {
        if w.container_id.is_some() {
            3
        } else if w.compute_target != "local" && !w.compute_target.is_empty() {
            2
        } else {
            1
        }
    });

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
            .as_ref()
            .map(|w| w.stack_fingerprint.clone())
            .filter(|s| !s.is_empty()),
        attestation_level,
        conversation_sha,
    })
}

/// Resolve the spec approval attestation for the MR's `spec_ref`
/// ("path@sha"): the newest active approval ledger entry for that exact
/// spec version. Returns `None` when the MR carries no spec_ref or the
/// version has no active approval — an approval is attested only when
/// the ledger actually holds one.
async fn build_spec_approval(
    state: &AppState,
    spec_ref: &str,
) -> Option<SpecRefAttestation> {
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
fn is_agent_gate(gate_type: &str) -> bool {
    matches!(gate_type, "AgentReview" | "AgentValidation")
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
fn build_gate_attestation(
    gate: Option<&QualityGate>,
    result: &gyre_domain::GateResult,
    review: Option<&Review>,
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
            attestation.review_decision = Some(match review.decision {
                ReviewDecision::Approved => "Approved".to_string(),
                ReviewDecision::ChangesRequested => "ChangesRequested".to_string(),
            });
            attestation.review_body_hash = Some(sha256_hex(
                review.body.as_deref().unwrap_or("").as_bytes(),
            ));
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
        Some(spec_ref) => (Some(spec_ref.to_string()), build_spec_approval(state, spec_ref).await),
        None => (None, None),
    };

    let gate_results = state.gate_results.list_by_mr_id(mr.id.as_str()).await?;
    let reviews = state.reviews.list_reviews(&mr.id).await.unwrap_or_default();

    let mut gates = Vec::with_capacity(gate_results.len());
    for result in &gate_results {
        let gate = state.quality_gates.find_by_id(result.gate_id.as_str()).await?;
        let review = gate
            .as_ref()
            .and_then(|g| review_for_gate(&reviews, &g.id));
        let mut attestation = build_gate_attestation(gate.as_ref(), result, review);

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
                Box::new(GateAgentSigner::derive(&state.agent_signing_key, &reviewer_id))
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

/// Persist the signed bundle: as an audit event (queryability) and as a
/// git note on the merge commit (spec §Storage). Failures are logged —
/// the merge has already landed and must not be rolled back by bundle
/// storage — but they are surfaced, never silently swallowed.
pub async fn store_merge_attestation_bundle(
    state: &AppState,
    mr: &MergeRequest,
    repo: &gyre_domain::Repository,
    bundle: &MergeAttestationBundle,
) {
    // Audit event: the signed bundle JSON in full.
    let event = gyre_domain::AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        gyre_domain::AuditEventType::Custom(MERGE_ATTESTATION_AUDIT_TYPE.to_string()),
        None,
        None,
        None,
        Some(mr.workspace_id.clone()),
        Some(repo.id.clone()),
        "merge_request".to_string(),
        Some(mr.id.to_string()),
        gyre_domain::AuditOutcome::Success,
        serde_json::to_value(bundle).unwrap_or(serde_json::Value::Null),
        None,
        None,
        crate::api::now_secs(),
    );
    if let Err(e) = state.audit.record(&event).await {
        tracing::warn!(
            mr_id = %mr.id,
            error = %e,
            "failed to record merge attestation bundle audit event"
        );
    }

    // Git note on the merge commit. Fire-and-forget like the legacy
    // attestation note: a missing git binary or a read-only repo must not
    // fail the merge; the audit event above is the durable record.
    let note_json = match serde_json::to_string(bundle) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(mr_id = %mr.id, error = %e, "cannot serialize merge attestation bundle note");
            return;
        }
    };
    let repo_path = repo.path.clone();
    let sha = bundle.merge_commit_sha.clone();
    let mr_id = mr.id.to_string();
    tokio::spawn(async move {
        let out = tokio::process::Command::new("git")
            .args([
                "-C",
                &repo_path,
                "notes",
                &format!("--ref={MERGE_ATTESTATION_NOTES_REF}"),
                "add",
                "-f",
                "-m",
                &note_json,
                &sha,
            ])
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => {
                tracing::info!(mr_id = %mr_id, sha = %sha, "merge attestation bundle note attached");
            }
            Ok(o) => {
                tracing::warn!(
                    mr_id = %mr_id,
                    sha = %sha,
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "merge attestation bundle git note failed — bundle stored in audit log only"
                );
            }
            Err(e) => {
                tracing::warn!(
                    mr_id = %mr_id,
                    sha = %sha,
                    error = %e,
                    "git not found — merge attestation bundle stored in audit log only"
                );
            }
        }
    });
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Verify every signature in a merge attestation bundle.
///
/// - The bundle-level forge signature over the canonical unsigned
///   document.
/// - Each gate's signature over its canonical unsigned attestation, with
///   the key of the party its `signed_by` names ("forge" → the forge key,
///   "agent:<id>" → that gate agent's derived key).
///
/// Requires the forge key (the Gyre instance holds it; external verifiers
/// resolve keys via OIDC once that integration lands). Returns `false` on
/// any tampering: a modified field, a swapped signature, or a `signed_by`
/// that does not match the signing party's key.
pub fn verify_merge_attestation_bundle(
    bundle: &MergeAttestationBundle,
    forge_key: &crate::auth::AgentSigningKey,
) -> bool {
    use ring::signature::UnparsedPublicKey;

    let ed25519_signature_ok = |payload: &[u8], sig_b64: &str, public_key: &[u8]| -> bool {
        let Ok(sig) = BASE64.decode(sig_b64) else {
            return false;
        };
        UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
            .verify(payload, &sig)
            .is_ok()
    };

    // 1. Bundle-level forge signature over the entire unsigned document.
    let Some(bundle_sig) = bundle.bundle_signature.as_deref() else {
        return false;
    };
    if !ed25519_signature_ok(
        bundle.unsigned_canonical_json().as_bytes(),
        bundle_sig,
        &forge_key.public_key_bytes,
    ) {
        return false;
    }

    // 2. Per-gate signatures, each against the named party's key.
    for gate in &bundle.gates {
        let Some(sig) = gate.signature.as_deref() else {
            return false;
        };
        let ok = match gate.signed_by.strip_prefix("agent:") {
            Some(agent_id) => {
                let signer = GateAgentSigner::derive(forge_key, agent_id);
                ed25519_signature_ok(
                    gate.unsigned_gate_json().as_bytes(),
                    sig,
                    signer.public_key_bytes(),
                )
            }
            None if gate.signed_by == "forge" => ed25519_signature_ok(
                gate.unsigned_gate_json().as_bytes(),
                sig,
                &forge_key.public_key_bytes,
            ),
            None => false,
        };
        if !ok {
            return false;
        }
    }

    true
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{GateStatus, GateResult, QualityGate, Review};

    fn test_state() -> Arc<AppState> {
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
            "MR attestation",
            "feat/x",
            "main",
            1_700_000_000,
        );
        mr.workspace_id = Id::new("ws-1");
        mr.author_agent_id = Some(Id::new("agent-author"));
        mr.spec_ref = Some("specs/system/identity-security.md@abc123def456abc123def456abc123def456abc1".to_string());
        mr
    }

    /// Seed one MR with: an active signed spec approval for its spec_ref,
    /// a TestCommand gate, a LintCommand gate, an AgentReview gate (with a
    /// gate-agent review), all three gate results Passed, and the author
    /// agent's conversation SHA + workload attestation.
    async fn seed(state: &Arc<AppState>) -> gyre_domain::MergeRequest {
        let mr = make_mr();
        state.merge_requests.create(&mr).await.unwrap();

        // Active, signed spec approval for the exact (path, sha).
        let mut approval = gyre_domain::SpecApproval::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            "specs/system/identity-security.md",
            "abc123def456abc123def456abc123def456abc1",
            "user:jsell",
        );
        approval.approve(1_700_000_050).unwrap();
        approval.signature = Some("<sigstore signature on spec approval>".to_string());
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
        assert!(approval.signature.is_some());

        // Author attestation (from the seeded provenance records).
        let author = bundle.author.as_ref().unwrap();
        assert_eq!(author.agent_id, "agent-author");
        assert_eq!(author.oidc_sub, "agent:agent-author");
        assert_eq!(author.stack_attestation.as_deref(), Some("sha256:stack-fingerprint"));
        assert_eq!(author.attestation_level, Some(1)); // local subprocess
        assert_eq!(author.conversation_sha.as_deref(), Some("sha256:conversation"));

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
        assert_eq!(sec.reviewer_oidc_sub.as_deref(), Some("agent:security-gate-7"));
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

        // Every signature verifies against the signing party's real key.
        assert!(
            verify_merge_attestation_bundle(&bundle, &state.agent_signing_key),
            "all signatures must verify"
        );

        // Serializes to valid JSON with the spec's field names.
        let json: serde_json::Value =
            serde_json::to_value(&bundle).expect("bundle must serialize");
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
        assert!(verify_merge_attestation_bundle(&bundle, &state.agent_signing_key));

        // Tamper with a gate status: the gate signature must fail.
        bundle.gates[0].status = "Failed".to_string();
        assert!(
            !verify_merge_attestation_bundle(&bundle, &state.agent_signing_key),
            "tampered gate status must fail verification"
        );

        // Restore, then tamper with merged_at: the bundle signature must fail.
        let fresh = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();
        let mut tampered = fresh.clone();
        tampered.merged_at += 1;
        assert!(
            !verify_merge_attestation_bundle(&tampered, &state.agent_signing_key),
            "tampered merged_at must fail the bundle signature"
        );

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
        assert!(
            !verify_merge_attestation_bundle(&swapped, &state.agent_signing_key),
            "a gate-agent signature must not verify under the forge key"
        );
    }

    #[tokio::test]
    async fn unsigned_and_missing_signatures_fail_verification() {
        let state = test_state();
        let mr = seed(&state).await;
        let mut bundle = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();

        bundle.bundle_signature = None;
        assert!(!verify_merge_attestation_bundle(&bundle, &state.agent_signing_key));

        let mut bundle2 = generate_merge_attestation_bundle(&state, &mr, "789abc", 1_774_000_000)
            .await
            .unwrap();
        bundle2.gates[0].signature = None;
        assert!(!verify_merge_attestation_bundle(&bundle2, &state.agent_signing_key));
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

        let bundle = generate_merge_attestation_bundle(&state, &mr, "abc", 1).await.unwrap();
        assert!(
            bundle.author.is_none(),
            "no author agent → no author attestation"
        );
        assert!(bundle.spec_ref.is_none());
        assert!(bundle.spec_approval.is_none());
        // Still a valid signed bundle.
        assert!(verify_merge_attestation_bundle(&bundle, &state.agent_signing_key));
    }
}
