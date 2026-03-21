//! Commit signature store for jj squash Sigstore signing (M13.8).
//!
//! After every `jj squash` the server signs the resulting commit SHA with the
//! forge's Ed25519 key and stores a `CommitSignature` record here.
//! The `GET /api/v1/repos/{id}/commits/{sha}/signature` endpoint returns the
//! stored record so callers can verify the signature against the JWKS endpoint.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Signing algorithm in use.  Currently only Ed25519 via the forge's OIDC key.
pub const ALGORITHM: &str = "EdDSA";

/// Mode of Sigstore signing.  Embedded in each `CommitSignature` record so
/// consumers know whether an external CA was involved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigstoreMode {
    /// Signed locally with the forge's Ed25519 key — no external Fulcio/Rekor.
    Local,
    /// Would use an external Fulcio CA (not yet configured; records a warning).
    Fulcio,
}

impl SigstoreMode {
    /// Parse from `GYRE_SIGSTORE_MODE` env var (default: `"local"`).
    pub fn from_env() -> Self {
        match std::env::var("GYRE_SIGSTORE_MODE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "fulcio" => {
                tracing::warn!(
                    "GYRE_SIGSTORE_MODE=fulcio: external Fulcio CA is not yet configured; \
                     falling back to local signing"
                );
                Self::Local
            }
            _ => Self::Local,
        }
    }
}

/// A signed record for one commit SHA produced by `jj squash`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSignature {
    /// The git commit SHA that was signed.
    pub commit_sha: String,
    /// Agent/user ID that triggered the squash (signer identity).
    pub signer_id: String,
    /// Signing algorithm (`"EdDSA"`).
    pub algorithm: String,
    /// Base64-encoded Ed25519 signature over the raw commit SHA bytes.
    pub signature: String,
    /// `kid` of the forge's Ed25519 key (matches `GET /.well-known/jwks.json`).
    pub signing_key_id: String,
    /// Unix epoch seconds when the signature was created.
    pub signed_at: u64,
    /// Mode used for signing.
    pub sigstore_mode: SigstoreMode,
}

/// In-memory store: commit SHA → `CommitSignature`.
pub type CommitSignatureStore = Arc<Mutex<HashMap<String, CommitSignature>>>;

/// Sign `commit_sha` with the forge's Ed25519 key and return a `CommitSignature`.
pub fn sign_commit(
    commit_sha: &str,
    signer_id: &str,
    signing_key: &crate::auth::AgentSigningKey,
    mode: SigstoreMode,
) -> CommitSignature {
    let raw_sig = signing_key.sign_bytes(commit_sha.as_bytes());
    CommitSignature {
        commit_sha: commit_sha.to_string(),
        signer_id: signer_id.to_string(),
        algorithm: ALGORITHM.to_string(),
        signature: BASE64.encode(&raw_sig),
        signing_key_id: signing_key.kid.clone(),
        signed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        sigstore_mode: mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fulcio_env_falls_back_to_local() {
        // When GYRE_SIGSTORE_MODE=fulcio the Fulcio CA is not configured, so
        // from_env() must return Local — not Fulcio — to accurately report the
        // actual signing method used.
        std::env::set_var("GYRE_SIGSTORE_MODE", "fulcio");
        let mode = SigstoreMode::from_env();
        std::env::remove_var("GYRE_SIGSTORE_MODE");
        assert_eq!(
            mode,
            SigstoreMode::Local,
            "GYRE_SIGSTORE_MODE=fulcio should fall back to Local, not Fulcio"
        );
    }

    #[test]
    fn local_env_returns_local() {
        std::env::set_var("GYRE_SIGSTORE_MODE", "local");
        let mode = SigstoreMode::from_env();
        std::env::remove_var("GYRE_SIGSTORE_MODE");
        assert_eq!(mode, SigstoreMode::Local);
    }

    #[test]
    fn default_env_returns_local() {
        std::env::remove_var("GYRE_SIGSTORE_MODE");
        assert_eq!(SigstoreMode::from_env(), SigstoreMode::Local);
    }
}

/// Verify a `CommitSignature` using the provided raw 32-byte Ed25519 public key.
///
/// Returns `true` if the signature is valid.
pub fn verify_commit_signature(record: &CommitSignature, public_key_bytes: &[u8]) -> bool {
    use ring::signature::{self, UnparsedPublicKey};
    let sig_bytes = match BASE64.decode(&record.signature) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);
    pk.verify(record.commit_sha.as_bytes(), &sig_bytes).is_ok()
}
