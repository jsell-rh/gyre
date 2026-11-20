//! Commit signing configuration and shared signing entry point
//! (identity-security.md §Layer 3, task-264).
//!
//! EVERY commit-producing surface (jj new, jj squash, git HTTP push, MR
//! merge) signs through [`sign_commit`], which enforces the task-264
//! invariants: keyless Fulcio signing by default, no silent fallback to
//! the local forge key, and a refusal (not an empty token) for callers
//! with no presentable OIDC identity. The resulting `CommitSignature`
//! record is persisted through `CommitSignatureRepository` and returned by
//! `GET /api/v1/repos/{id}/commits/{sha}/signature`.
//!
//! The cryptographic signature covers the canonical
//! [`signed_commit_payload`] — commit SHA + task_id + spawned_by — so the
//! attribution is bound into the signed (and Rekor-recorded) material,
//! not merely stored as detached plaintext columns.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
pub use gyre_ports::commit_signature_repo::{
    CommitSignature, SigstoreMode, ALGORITHM_ECDSA_P256, ALGORITHM_ED25519,
};
use serde::Serialize;

/// Default public Fulcio instance (fulcio.sigstore.dev).
pub const DEFAULT_FULCIO_URL: &str = "https://fulcio.sigstore.dev";
/// Default public Rekor instance (rekor.sigstore.dev).
pub const DEFAULT_REKOR_URL: &str = "https://rekor.sigstore.dev";

/// Per-request timeout for every outbound Fulcio/Rekor HTTP call (task-107 F10).
///
/// reqwest has no default total timeout; without this bound a hung signing
/// stack would stall the commit operation indefinitely — with task-264's
/// no-silent-fallback semantics that stall must resolve into an explicit
/// error within a bounded time, not hang forever.
pub const SIGNING_HTTP_TIMEOUT_SECS: u64 = 10;

/// Signing backend selection (`GYRE_SIGNING_MODE` / legacy `GYRE_SIGSTORE_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningMode {
    /// Local Ed25519 signing with the forge key — explicit operator
    /// configuration only (`GYRE_SIGNING_MODE=local`).
    Local,
    /// Sigstore keyless signing via Fulcio + Rekor — the default
    /// (identity-security.md §Layer 3: "no long-lived GPG keys to manage
    /// or rotate").
    Fulcio,
    /// Skip signing entirely — explicit operator opt-in only, never the
    /// default. Commits are produced with no signature record.
    None,
}

impl SigningMode {
    /// Parse a raw `GYRE_SIGNING_MODE` value.
    ///
    /// Unset/empty selects keyless Fulcio signing (task-264: keyless is
    /// the default, not an opt-in). `local` and `none` are explicit
    /// operator opt-ins. Any other value is a configuration ERROR — the
    /// previous behavior silently mapped unknown values to `local`, which
    /// made a typo'd mode silently downgrade the deployment to long-lived
    /// forge-key signing.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_lowercase().as_str() {
            "" | "fulcio" => Ok(Self::Fulcio),
            "local" => Ok(Self::Local),
            "none" => Ok(Self::None),
            other => Err(format!(
                "unknown GYRE_SIGNING_MODE value {other:?}: expected \"fulcio\" (the default), \
                 \"local\", or \"none\""
            )),
        }
    }

    /// Resolve the mode from the environment. Fails loudly (panics) on an
    /// unrecognized value: a misconfigured signing mode must stop the
    /// server at boot, not silently select a weaker signing backend.
    pub fn from_env() -> Self {
        let raw = std::env::var("GYRE_SIGNING_MODE")
            .or_else(|_| std::env::var("GYRE_SIGSTORE_MODE"))
            .unwrap_or_default();
        Self::parse(&raw).unwrap_or_else(|e| panic!("commit signing is misconfigured: {e}"))
    }
}

/// Parse a `GYRE_SIGNING_FULCIO_FALLBACK` value: only the exact value
/// `local` enables the operator-configured fallback from failed keyless
/// signing to local forge-key signing. Everything else (including unset)
/// leaves the fail-loud default in place.
pub fn parse_fulcio_fallback(raw: Option<&str>) -> bool {
    raw.is_some_and(|v| v.trim().eq_ignore_ascii_case("local"))
}

/// Whether the operator has explicitly configured
/// `GYRE_SIGNING_FULCIO_FALLBACK=local` (task-264). With the fallback
/// enabled, a Fulcio failure degrades to local signing — loudly (warn
/// log, and the stored record honestly carries `sigstore_mode=local`).
pub fn fulcio_fallback_local() -> bool {
    parse_fulcio_fallback(
        std::env::var("GYRE_SIGNING_FULCIO_FALLBACK")
            .ok()
            .as_deref(),
    )
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
///
/// The signature covers the canonical [`signed_commit_payload`] (commit
/// SHA + task_id + spawned_by), so the attribution is cryptographically
/// bound: tampering any of the three fields in the stored record breaks
/// verification.
pub fn sign_commit_local(
    repo_id: &str,
    commit_sha: &str,
    attribution: &SigningAttribution,
    signing_key: &crate::auth::AgentSigningKey,
    oidc_issuer: &str,
) -> CommitSignature {
    let raw_sig = signing_key.sign_bytes(&signed_commit_payload(
        commit_sha,
        &attribution.task_id,
        &attribution.spawned_by,
    ));
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
/// Ed25519 public key. Returns `true` if the signature is valid over the
/// canonical payload recomputed from the record's OWN commit_sha/task_id/
/// spawned_by fields — a record whose attribution was edited after signing
/// fails, because the payload no longer matches the signed bytes.
pub fn verify_commit_signature(record: &CommitSignature, public_key_bytes: &[u8]) -> bool {
    use ring::signature::{self, UnparsedPublicKey};
    let sig_bytes = match BASE64.decode(&record.signature) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);
    pk.verify(&record_signed_payload(record), &sig_bytes)
        .is_ok()
}

/// The canonical attribution-bound payload every commit signature covers
/// (task-264, identity-security.md §Layer 3: "the signature proves which
/// agent, on which task, spawned by which user, made the commit").
///
/// Canonical JSON with a fixed field order (serde struct serialization),
/// so sign and verify compute byte-identical payloads from the same
/// triple. Rekor entries digest the same bytes.
#[derive(Serialize)]
struct SignedCommitPayload<'a> {
    commit_sha: &'a str,
    task_id: &'a str,
    spawned_by: &'a str,
}

/// Build the canonical signed payload for a commit.
pub fn signed_commit_payload(commit_sha: &str, task_id: &str, spawned_by: &str) -> Vec<u8> {
    serde_json::to_vec(&SignedCommitPayload {
        commit_sha,
        task_id,
        spawned_by,
    })
    .expect("serializing three string fields cannot fail")
}

/// The canonical payload for a stored record, recomputed from the record's
/// own fields. Verification uses this: a record whose plaintext attribution
/// was tampered after signing yields a different payload and fails.
pub fn record_signed_payload(record: &CommitSignature) -> Vec<u8> {
    signed_commit_payload(&record.commit_sha, &record.task_id, &record.spawned_by)
}

// ── Shared signing entry point (task-264) ────────────────────────────────────

/// Why a commit could not be signed.
#[derive(Debug)]
pub enum CommitSigningError {
    /// Keyless (fulcio) signing was REFUSED: the caller has no presentable
    /// OIDC identity token. The old behavior presented an empty bearer
    /// token to Fulcio — guaranteeing failure and a silent fallback to the
    /// local forge key — so this is now an explicit refusal.
    NoOidcIdentity(String),
    /// The configured signing stack failed. The message names the active
    /// mode; no local-key record was produced (unless the operator
    /// explicitly configured `GYRE_SIGNING_FULCIO_FALLBACK=local`).
    SigningStackFailed {
        mode: &'static str,
        source: anyhow::Error,
    },
}

impl CommitSigningError {
    /// Human-readable description naming the active signing mode where one
    /// is known — surfaces in API responses, push rejections, and merge
    /// failure reasons so the operator sees WHICH mode actually ran.
    pub fn describe(&self) -> String {
        match self {
            Self::NoOidcIdentity(reason) => reason.clone(),
            Self::SigningStackFailed { mode, source } => {
                format!("commit signing failed (signing mode '{mode}'): {source:#}")
            }
        }
    }
}

/// Sign one commit through the server's configured signing mode — the
/// single entry point every commit-producing surface uses (task-264).
///
/// - `None` mode: returns `Ok(None)` (unsigned — the explicit operator
///   opt-in; never the default).
/// - `Local` mode: signs with the forge Ed25519 key.
/// - `Fulcio` mode (default): keyless signing against the caller's OIDC
///   JWT. A caller with no presentable JWT is refused with
///   [`CommitSigningError::NoOidcIdentity`] — an empty/non-JWT bearer is
///   never presented to Fulcio. A Fulcio/Rekor failure fails loudly;
///   the ONLY fallback is the operator-configured
///   `GYRE_SIGNING_FULCIO_FALLBACK=local`, which signs locally and is
///   visible in the record's `sigstore_mode=local` plus a warn log.
pub async fn sign_commit(
    state: &crate::AppState,
    repo_id: &str,
    commit_sha: &str,
    attribution: &SigningAttribution,
    oidc_jwt: Option<&str>,
) -> Result<Option<CommitSignature>, CommitSigningError> {
    match state.signing_config.mode {
        SigningMode::None => Ok(None),
        SigningMode::Local => Ok(Some(sign_commit_local(
            repo_id,
            commit_sha,
            attribution,
            &state.agent_signing_key,
            &state.base_url,
        ))),
        SigningMode::Fulcio => {
            // Refuse — never present an empty or non-JWT bearer to Fulcio
            // (the old empty-token path guaranteed failure + fallback).
            let jwt = oidc_jwt
                .filter(|t| crate::sigstore::jwt_is_presentable(t))
                .ok_or_else(|| {
                    CommitSigningError::NoOidcIdentity(format!(
                        "keyless (fulcio) signing refused for signer {:?}: the caller has no \
                         presentable OIDC identity token; use an OIDC-issued agent JWT, or set \
                         GYRE_SIGNING_MODE=local for explicit forge-key signing",
                        attribution.agent_id
                    ))
                })?;
            let transport = crate::sigstore::production_transport(state.http_client.clone());
            match crate::sigstore::sign_commit_keyless(
                repo_id,
                commit_sha,
                attribution,
                jwt,
                &state.signing_config,
                transport.as_ref(),
            )
            .await
            {
                Ok(signed) => Ok(Some(signed.record)),
                Err(source) => {
                    if fulcio_fallback_local() {
                        tracing::warn!(
                            error = %source,
                            commit_sha,
                            signer = %attribution.agent_id,
                            "fulcio keyless signing failed; GYRE_SIGNING_FULCIO_FALLBACK=local \
                             is configured — signing with the local forge key (the stored record \
                             carries sigstore_mode=local)"
                        );
                        Ok(Some(sign_commit_local(
                            repo_id,
                            commit_sha,
                            attribution,
                            &state.agent_signing_key,
                            &state.base_url,
                        )))
                    } else {
                        Err(CommitSigningError::SigningStackFailed {
                            mode: "fulcio",
                            source,
                        })
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_mode_default_and_explicit_values() {
        // Unset (empty) selects keyless Fulcio — the task-264 default.
        assert_eq!(SigningMode::parse("").unwrap(), SigningMode::Fulcio);
        assert_eq!(SigningMode::parse("fulcio").unwrap(), SigningMode::Fulcio);
        assert_eq!(SigningMode::parse(" Fulcio ").unwrap(), SigningMode::Fulcio);
        // Local and none are explicit operator opt-ins only.
        assert_eq!(SigningMode::parse("local").unwrap(), SigningMode::Local);
        assert_eq!(SigningMode::parse("none").unwrap(), SigningMode::None);
        // An unrecognized value is a configuration error, never a silent
        // downgrade to local signing (the old `_ => Self::Local` arm).
        let err = SigningMode::parse("locall").unwrap_err();
        assert!(err.contains("unknown GYRE_SIGNING_MODE"), "got: {err}");
        assert!(SigningMode::parse("keyless").is_err());
    }

    #[test]
    fn fulcio_fallback_requires_explicit_local_value() {
        assert!(!parse_fulcio_fallback(None));
        assert!(!parse_fulcio_fallback(Some("")));
        assert!(!parse_fulcio_fallback(Some("warn")));
        assert!(parse_fulcio_fallback(Some("local")));
        assert!(parse_fulcio_fallback(Some("LOCAL")));
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

    /// task-264: attribution is part of the SIGNED material — editing the
    /// stored task_id (or swapping another task's record fields in) breaks
    /// verification, because the payload recomputed from the record's own
    /// fields no longer matches the signed bytes.
    #[test]
    fn local_verify_fails_on_tampered_task_attribution() {
        let key = crate::auth::AgentSigningKey::generate();
        let attr = SigningAttribution {
            agent_id: "a".into(),
            task_id: "task-genuine".into(),
            spawned_by: "user-genuine".into(),
        };
        let mut rec = sign_commit_local("repo-1", "abc123", &attr, &key, "http://issuer");
        rec.task_id = "task-forged".to_string();
        assert!(
            !verify_commit_signature(&rec, &key.public_key_bytes),
            "a tampered task_id must fail verification — attribution is signed, \
             not a plaintext-only column"
        );

        // Same for spawned_by.
        let mut rec2 = sign_commit_local("repo-1", "abc123", &attr, &key, "http://issuer");
        rec2.spawned_by = "user-forged".to_string();
        assert!(!verify_commit_signature(&rec2, &key.public_key_bytes));
    }

    /// The canonical payload is deterministic (fixed field order) and
    /// differs for every distinct attribution — sign and verify compute
    /// identical bytes from the same triple.
    #[test]
    fn signed_payload_is_canonical_and_attribution_sensitive() {
        let a = signed_commit_payload("sha", "task-1", "user-1");
        let b = signed_commit_payload("sha", "task-1", "user-1");
        assert_eq!(a, b);
        assert_ne!(a, signed_commit_payload("sha", "task-2", "user-1"));
        assert_ne!(a, signed_commit_payload("sha2", "task-1", "user-1"));
        assert_ne!(a, signed_commit_payload("sha", "task-1", "user-2"));
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
            auth_mechanism: crate::auth::AuthMechanism::AgentToken,
            agent_id: "agent-42".into(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".into(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: Some(serde_json::json!({
                "sub": "agent-42",
                "task_id": "task-107",
                "spawned_by": "user-jsell",
            })),
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let attr = SigningAttribution::from_auth(&auth);
        assert_eq!(attr.agent_id, "agent-42");
        assert_eq!(attr.task_id, "task-107");
        assert_eq!(attr.spawned_by, "user-jsell");
    }

    #[test]
    fn attribution_without_claims_uses_empty_not_fabricated() {
        let auth = crate::auth::AuthenticatedAgent {
            auth_mechanism: crate::auth::AuthMechanism::GlobalToken,
            agent_id: "system".into(),
            user_id: None,
            roles: vec![],
            tenant_id: "default".into(),
            workspace_id: None,
            repo_id: None,
            scope: Vec::new(),
            jwt_claims: None,
            bearer_token: None,
            deprecated_token_auth: false,
            impersonation: None,
        };
        let attr = SigningAttribution::from_auth(&auth);
        assert_eq!(attr.agent_id, "system");
        assert_eq!(attr.task_id, "");
        assert_eq!(attr.spawned_by, "");
    }
}
