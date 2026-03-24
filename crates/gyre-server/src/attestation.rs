//! Merge attestation bundles (G5).
//!
//! After every successful merge the merge processor assembles a `MergeAttestation`
//! record containing the MR ID, merge commit SHA, gate results, spec approval
//! status, and author identity.  The record is canonicalised to JSON, signed with
//! the server's Ed25519 key, wrapped in an `AttestationBundle`, stored in the
//! in-memory `attestation_store`, and attached to the merge commit as a git note
//! under `refs/notes/attestations`.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

pub use gyre_domain::{AttestationBundle, AttestationGateResult, MergeAttestation};

// ── Signing ──────────────────────────────────────────────────────────────────

/// Sign `attestation` with `signing_key` and return an `AttestationBundle`.
///
/// The canonical form is deterministic JSON (struct field order as declared).
/// The signature covers the UTF-8 bytes of that JSON.
pub fn sign_attestation(
    attestation: MergeAttestation,
    signing_key: &crate::auth::AgentSigningKey,
) -> AttestationBundle {
    let canonical =
        serde_json::to_string(&attestation).expect("MergeAttestation serialisation must not fail");
    let raw_sig = signing_key.sign_bytes(canonical.as_bytes());
    let signature = BASE64.encode(&raw_sig);
    let signing_key_id = signing_key.kid.clone();
    AttestationBundle {
        attestation,
        signature,
        signing_key_id,
    }
}

/// Verify a bundle's signature using the provided raw 32-byte Ed25519 public key.
///
/// Returns `true` if the signature is valid.
pub fn verify_bundle(bundle: &AttestationBundle, public_key_bytes: &[u8]) -> bool {
    use ring::signature::{self, UnparsedPublicKey};
    let canonical = match serde_json::to_string(&bundle.attestation) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let sig_bytes = match BASE64.decode(&bundle.signature) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);
    pk.verify(canonical.as_bytes(), &sig_bytes).is_ok()
}
