//! Port trait for commit signature persistence (identity-security.md §Layer 3).
//!
//! `jj squash` signs the resulting commit (local Ed25519 or a Fulcio-issued
//! short-lived certificate) and stores a `CommitSignature` record. The record
//! is the platform's copy of the cryptographic proof — the Rekor entry alone
//! does not carry the certificate chain or the attribution claims — so it must
//! survive a server restart. Implementations are keyed by `(repo_id, commit_sha)`
//! so a record created for one repo is not retrievable through another repo's
//! API path.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Signing algorithm in use for the stored signature.
pub const ALGORITHM_ED25519: &str = "EdDSA";
/// ECDSA P-256/SHA-256 (ASN.1 DER signature) — the algorithm Fulcio issues for.
pub const ALGORITHM_ECDSA_P256: &str = "ECDSA_P256_SHA256";

/// Mode used to sign a commit (identity-security.md §Layer 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigstoreMode {
    /// Signed locally with the forge's Ed25519 key — no external Fulcio/Rekor.
    Local,
    /// Signed with a short-lived certificate issued by Fulcio; the signature
    /// and certificate are recorded in Rekor for non-repudiation.
    Fulcio,
}

/// A signed record for one commit produced by `jj squash`.
///
/// Local mode populates `signature` (base64 Ed25519 over the commit SHA) and
/// `signing_key_id`. Fulcio mode populates `signature` (base64 DER ECDSA over
/// the commit SHA), `certificate_pem` (the leaf certificate), `certificate_chain_pem`
/// (intermediates + root, leaf first after the leaf itself is excluded — the
/// chain exactly as Fulcio returned it, leaf included), `rekor_entry_id`, and
/// the attribution claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSignature {
    /// Repo the commit belongs to. Part of the storage key.
    pub repo_id: String,
    /// The git commit SHA that was signed.
    pub commit_sha: String,
    /// Agent/user ID that triggered the squash (signer identity).
    pub signer_id: String,
    /// Task the signing agent was assigned to (from the agent JWT `task_id`).
    pub task_id: String,
    /// Identity that spawned the signing agent (from the agent JWT `spawned_by`).
    pub spawned_by: String,
    /// Signing algorithm (`"EdDSA"` or `"ECDSA_P256_SHA256"`).
    pub algorithm: String,
    /// Base64-encoded signature over the commit SHA bytes.
    pub signature: String,
    /// `kid` of the forge's Ed25519 key (local mode only).
    pub signing_key_id: String,
    /// Unix epoch seconds when the signature was created.
    pub signed_at: u64,
    /// Mode used for signing.
    pub sigstore_mode: SigstoreMode,
    /// OIDC subject bound into the Fulcio certificate SAN/CN (Fulcio mode).
    pub oidc_subject: String,
    /// OIDC issuer of the JWT used to request the certificate (Fulcio mode).
    pub oidc_issuer: String,
    /// PEM-encoded leaf certificate (Fulcio mode only).
    pub certificate_pem: Option<String>,
    /// PEM-encoded chain as returned by Fulcio: leaf first, then intermediates,
    /// then root (Fulcio mode only).
    pub certificate_chain_pem: Option<String>,
    /// Rekor log entry UUID (Fulcio mode only).
    pub rekor_entry_id: Option<String>,
}

/// Repository for commit signatures produced by `jj squash`.
///
/// Records are keyed by `(repo_id, commit_sha)`; a lookup through repo A never
/// returns a record stored for repo B.
#[async_trait]
pub trait CommitSignatureRepository: Send + Sync {
    /// Store (upsert) a signature record.
    async fn save(&self, signature: &CommitSignature) -> Result<()>;

    /// Find the signature record for a commit within a repo.
    async fn find(
        &self,
        repo_id: &str,
        commit_sha: &str,
    ) -> Result<Option<CommitSignature>>;
}
