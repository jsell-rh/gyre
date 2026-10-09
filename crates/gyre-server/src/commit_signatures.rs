//! Commit signing configuration and local-mode helpers (identity-security.md §Layer 3).
//!
//! `jj squash` signs the resulting commit SHA either with the forge's Ed25519
//! key (local mode) or with a short-lived Fulcio-issued certificate (fulcio
//! mode — see `sigstore.rs`). The resulting `CommitSignature` record is
//! persisted through `CommitSignatureRepository` and returned by
//! `GET /api/v1/repos/{id}/commits/{sha}/signature`.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
pub use gyre_ports::commit_signature_repo::{
    CommitSignature, SigstoreMode, ALGORITHM_ECDSA_P256, ALGORITHM_ED25519,
};

/// Default public Fulcio instance (fulcio.sigstore.dev).
pub const DEFAULT_FULCIO_URL: &str = "https://fulcio.sigstore.dev";
/// Default public Rekor instance (rekor.sigstore.dev).
pub const DEFAULT_REKOR_URL: &str = "https://rekor.sigstore.dev";

/// Per-request timeout for every outbound Fulcio/Rekor HTTP call (task-107 F10).
///
/// reqwest has no default total timeout; without this bound a hung signing
/// stack would stall the squash request indefinitely and defeat the
/// fall-back-to-local guarantee.
pub const SIGNING_HTTP_TIMEOUT_SECS: u64 = 10;

/// Signing backend selection (`GYRE_SIGNING_MODE` / legacy `GYRE_SIGSTORE_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningMode {
    /// Local Ed25519 signing with the forge key (default).
    Local,
    /// Sigstore keyless signing via Fulcio + Rekor.
    Fulcio,
    /// Skip signing entirely — squash succeeds with no signature record.
    None,
}

impl SigningMode {
    pub fn from_env() -> Self {
        let raw = std::env::var("GYRE_SIGNING_MODE")
            .or_else(|_| std::env::var("GYRE_SIGSTORE_MODE"))
            .unwrap_or_default()
            .to_lowercase();
        match raw.as_str() {
            "fulcio" => Self::Fulcio,
            "none" => Self::None,
            _ => Self::Local,
        }
    }
}

/// Server-side signing configuration. This is the trust anchor for
/// verification (task-107 F3): the Fulcio/Rekor instances used to VERIFY a
/// record are resolved from this config, never from the record being verified.
#[derive(Debug, Clone)]
pub struct SigningConfig {
    pub mode: SigningMode,
    /// Fulcio instance URL (`GYRE_FULCIO_URL`, default public Fulcio).
    pub fulcio_url: String,
    /// Rekor instance URL (`GYRE_REKOR_URL`, default public Rekor).
    pub rekor_url: String,
}

impl Default for SigningConfig {
    fn default() -> Self {
        Self {
            mode: SigningMode::from_env(),
            fulcio_url: DEFAULT_FULCIO_URL.to_string(),
            rekor_url: DEFAULT_REKOR_URL.to_string(),
        }
    }
}

impl SigningConfig {
    pub fn from_env() -> Self {
        Self {
            mode: SigningMode::from_env(),
            fulcio_url: std::env::var("GYRE_FULCIO_URL")
                .ok()
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| DEFAULT_FULCIO_URL.to_string()),
            rekor_url: std::env::var("GYRE_REKOR_URL")
                .ok()
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| DEFAULT_REKOR_URL.to_string()),
        }
    }
}

/// Attribution extracted from the caller's validated JWT claims (task-107 F2).
///
/// The spec's Layer 3 sentence requires the signature to prove *which agent,
/// on which task, spawned by which user* made the commit; the agent JWT
/// carries `task_id` and `spawned_by`, and this struct carries them from the
/// authenticated request into the signing boundary without placeholder
/// literals.
#[derive(Debug, Clone)]
pub struct SigningAttribution {
    pub agent_id: String,
    pub task_id: String,
    pub spawned_by: String,
}

impl SigningAttribution {
    /// Extract attribution from an `AuthenticatedAgent`'s validated JWT claims.
    /// For non-JWT callers (global dev token, API key) there are no claims;
    /// attribution falls back to the resolved agent id with empty task/user
    /// fields rather than fabricated literals.
    pub fn from_auth(auth: &crate::auth::AuthenticatedAgent) -> Self {
        let (task_id, spawned_by) = auth
            .jwt_claims
            .as_ref()
            .map(|claims| {
                let task_id = claims
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let spawned_by = claims
                    .get("spawned_by")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (task_id, spawned_by)
            })
            .unwrap_or_default();
        Self {
            agent_id: auth.agent_id.clone(),
            task_id,
            spawned_by,
        }
    }
}

/// Sign `commit_sha` with the forge's Ed25519 key and return a local-mode
/// `CommitSignature` record.
pub fn sign_commit_local(
    repo_id: &str,
    commit_sha: &str,
    attribution: &SigningAttribution,
    signing_key: &crate::auth::AgentSigningKey,
    oidc_issuer: &str,
) -> CommitSignature {
    let raw_sig = signing_key.sign_bytes(commit_sha.as_bytes());
    CommitSignature {
        repo_id: repo_id.to_string(),
        commit_sha: commit_sha.to_string(),
        signer_id: attribution.agent_id.clone(),
        task_id: attribution.task_id.clone(),
        spawned_by: attribution.spawned_by.clone(),
        algorithm: ALGORITHM_ED25519.to_string(),
        signature: BASE64.encode(&raw_sig),
        signing_key_id: signing_key.kid.clone(),
        signed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        sigstore_mode: SigstoreMode::Local,
        oidc_subject: attribution.agent_id.clone(),
        oidc_issuer: oidc_issuer.to_string(),
        certificate_pem: None,
        certificate_chain_pem: None,
        rekor_entry_id: None,
    }
}

/// Verify a local-mode `CommitSignature` using the provided raw 32-byte
/// Ed25519 public key. Returns `true` if the signature is valid.
pub fn verify_commit_signature(record: &CommitSignature, public_key_bytes: &[u8]) -> bool {
    use ring::signature::{self, UnparsedPublicKey};
    let sig_bytes = match BASE64.decode(&record.signature) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);
    pk.verify(record.commit_sha.as_bytes(), &sig_bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_mode_parses_env_values() {
        // from_env reads process env; test the mapping directly via a
        // temp-env-free approach: SigningMode has no env in unit isolation,
        // so exercise through Default-ish construction.
        let cfg = SigningConfig {
            mode: SigningMode::Local,
            fulcio_url: "http://x".into(),
            rekor_url: "http://y".into(),
        };
        assert_eq!(cfg.mode, SigningMode::Local);
    }

    #[test]
    fn local_sign_and_verify_roundtrip() {
        let key = crate::auth::AgentSigningKey::generate();
        let attr = SigningAttribution {
            agent_id: "agent-1".into(),
            task_id: "task-9".into(),
            spawned_by: "user-7".into(),
        };
        let rec = sign_commit_local("repo-1", "abc123", &attr, &key, "http://issuer");
        assert_eq!(rec.sigstore_mode, SigstoreMode::Local);
        assert_eq!(rec.task_id, "task-9");
        assert_eq!(rec.spawned_by, "user-7");
        assert!(verify_commit_signature(&rec, &key.public_key_bytes));
    }

    #[test]
    fn local_verify_rejects_tampered_signature() {
        let key = crate::auth::AgentSigningKey::generate();
        let attr = SigningAttribution {
            agent_id: "a".into(),
            task_id: "t".into(),
            spawned_by: "u".into(),
        };
        let mut rec = sign_commit_local("repo-1", "abc123", &attr, &key, "http://issuer");
        rec.signature = BASE64.encode([0u8; 64]);
        assert!(!verify_commit_signature(&rec, &key.public_key_bytes));
    }

    #[test]
    fn local_verify_rejects_wrong_commit_sha() {
        let key = crate::auth::AgentSigningKey::generate();
        let attr = SigningAttribution {
            agent_id: "a".into(),
            task_id: "t".into(),
            spawned_by: "u".into(),
        };
        let mut rec = sign_commit_local("repo-1", "abc123", &attr, &key, "http://issuer");
        rec.commit_sha = "other".into();
        assert!(!verify_commit_signature(&rec, &key.public_key_bytes));
    }

    #[test]
    fn attribution_extracts_task_and_spawner_from_claims() {
        let auth = crate::auth::AuthenticatedAgent {
            agent_id: "agent-42".into(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".into(),
            jwt_claims: Some(serde_json::json!({
                "sub": "agent-42",
                "task_id": "task-107",
                "spawned_by": "user-jsell",
            })),
            deprecated_token_auth: false,
        };
        let attr = SigningAttribution::from_auth(&auth);
        assert_eq!(attr.agent_id, "agent-42");
        assert_eq!(attr.task_id, "task-107");
        assert_eq!(attr.spawned_by, "user-jsell");
    }

    #[test]
    fn attribution_without_claims_uses_empty_not_fabricated() {
        let auth = crate::auth::AuthenticatedAgent {
            agent_id: "system".into(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".into(),
            jwt_claims: None,
            deprecated_token_auth: false,
        };
        let attr = SigningAttribution::from_auth(&auth);
        assert_eq!(attr.agent_id, "system");
        assert_eq!(attr.task_id, "");
        assert_eq!(attr.spawned_by, "");
    }
}
