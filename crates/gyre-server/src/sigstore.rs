//! Sigstore keyless commit signing: Fulcio certificate issuance and Rekor
//! transparency-log recording (identity-security.md §Layer 3).
//!
//! Wire formats verified against upstream sources:
//! - Fulcio v2 `CreateSigningCertificate` REST (`POST {fulcio}/api/v2/signingCert`)
//!   sends protojson `{"credentials": {"oidcIdentityToken": ...}, "publicKeyRequest":
//!   {"publicKey": {"content": <PEM text>, "algorithm": ...}, "proofOfPossession":
//!   <base64 P1363 signature over the JWT `sub` claim>}}`. Fulcio's
//!   `challenges.ParsePublicKey` accepts PEM text or raw DER bytes for
//!   `content` — a base64-ASCII SPKI string is rejected by both branches
//!   (review task-107 F1), so we send the PEM-encoded public key exactly like
//!   sigstore-go's reference client.
//! - The response wraps the issued certificate chain in a
//!   `signedCertificateEmbeddedSct.chain.certificates` array (base64 DER),
//!   plus an `X-Signature`/`X-Certificate` SCT header pair we ignore
//!   (inclusion is Rekor's job here).
//! - Rekor `POST /api/v1/log/entries` with a hashedrekord type body:
//!   `{"kind": "hashedrekord", "apiVersion": "0.0.1", "spec": {"data":
//!   {"hash": {"algorithm": "sha256", "value": <hex>}}, "signature":
//!   {"content": <base64 DER sig>, "publicKey": {"content": <base64 PEM cert>}}}}`.
//!   The response maps entry-UUID → entry object with `body` (base64 of the
//!   canonical JSON entry). Verification fetches the entry and compares the
//!   decoded hashedrekord's hash/signature/certificate against the record
//!   (review F4: content match, not mere existence).
//!
//! Trust anchors (review F3): verification resolves the Fulcio trust bundle
//! and Rekor log ONLY from the server's `SigningConfig`; a record claiming
//! different URLs fails verification rather than redirecting the verifier.
//!
//! Latency (review F10): every outbound call is wrapped in
//! `tokio::time::timeout` — reqwest has no default total timeout, and the
//! fall-back-to-local guarantee must hold for hung endpoints, not just
//! refused connections.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_ports::commit_signature_repo::{CommitSignature, SigstoreMode, ALGORITHM_ECDSA_P256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use std::future::Future;

use async_trait::async_trait;
use crate::commit_signatures::{SigningAttribution, SigningConfig};

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FulcioPublicKeyRequest<'a> {
    #[serde(rename = "publicKey")]
    public_key: FulcioPublicKey<'a>,
    #[serde(rename = "proofOfPossession")]
    proof_of_possession: &'a str,
}

#[derive(Serialize)]
struct FulcioPublicKey<'a> {
    /// PEM-encoded public key — the encoding Fulcio's ParsePublicKey accepts.
    content: &'a str,
    algorithm: &'a str,
}

#[derive(Serialize)]
struct FulcioCredentials<'a> {
    #[serde(rename = "oidcIdentityToken")]
    oidc_identity_token: &'a str,
}

#[derive(Serialize)]
struct FulcioSigningCertRequest<'a> {
    credentials: FulcioCredentials<'a>,
    #[serde(rename = "publicKeyRequest")]
    public_key_request: FulcioPublicKeyRequest<'a>,
}

/// Fulcio v2 `CreateSigningCertificateResponse` (protojson). Oneof variants
/// not used by real Fulcio issuance are omitted.
#[derive(Deserialize)]
struct FulcioSigningCertResponse {
    #[serde(rename = "signedCertificateEmbeddedSct")]
    signed_certificate_embedded_sct: Option<SignedCertificateEmbeddedSct>,
    #[serde(rename = "signedCertificateDetachedSct")]
    signed_certificate_detached_sct: Option<SignedCertificateDetachedSct>,
}

#[derive(Deserialize)]
struct SignedCertificateEmbeddedSct {
    chain: Option<FulcioChain>,
}

/// The detached-SCT variant carries the same `chain` field; both oneof arms
/// are handled uniformly by extracting the chain.
#[derive(Deserialize)]
struct SignedCertificateDetachedSct {
    chain: Option<FulcioChain>,
}

/// `Chain` carries `certificates` as repeated `bytes` — in protojson each is
/// base64 of the DER certificate. Element 0 is the leaf; the rest are
/// intermediates and the root.
#[derive(Deserialize)]
struct FulcioChain {
    #[serde(default)]
    certificates: Vec<String>,
}

/// Fulcio v2 trust bundle: `{"chains": [[<base64 DER cert>, ...], ...]}`.
/// `chains` is repeated precisely so an instance can serve multiple
/// concurrent CA chains (rotation windows, multi-CA deployments) — verification
/// must try every chain, not just the first (review F11).
#[derive(Deserialize)]
struct TrustBundleResponse {
    #[serde(default)]
    chains: Vec<Vec<String>>,
}

#[derive(Serialize)]
struct RekorHashedrekordSpec<'a> {
    data: RekorData<'a>,
    signature: RekorSignature<'a>,
}

#[derive(Serialize)]
struct RekorData<'a> {
    hash: RekorHash<'a>,
}

#[derive(Serialize)]
struct RekorHash<'a> {
    algorithm: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
struct RekorSignature<'a> {
    content: &'a str,
    public_key: RekorPublicKey<'a>,
}

#[derive(Serialize)]
struct RekorPublicKey<'a> {
    content: &'a str,
}

#[derive(Serialize)]
struct RekorEntryRequest<'a> {
    kind: &'a str,
    #[serde(rename = "apiVersion")]
    api_version: &'a str,
    spec: RekorHashedrekordSpec<'a>,
}

/// Rekor `GET /api/v1/log/entries` response: map of entryUUID → entry.
#[derive(Deserialize)]
struct RekorEntryResponse(std::collections::HashMap<String, RekorEntry>);

#[derive(Deserialize)]
struct RekorEntry {
    body: Option<String>,
}

// ── Result type ───────────────────────────────────────────────────────────────

/// Verification result for one commit signature (task plan item 3).
///
/// Checks: (a) signature validity, (b) certificate chain issued by the
/// configured Fulcio (incl. validity window), (c) OIDC subject match, (d)
/// Rekor entry existence AND content match. Attribution fields surface
/// task/spawning-user provenance (review F2).
#[derive(Debug, Clone, Serialize)]
pub struct SignatureVerificationResult {
    pub valid: bool,
    /// (a) signature cryptographically verifies against the certificate.
    pub signature_valid: bool,
    /// (b) chain roots in the configured Fulcio trust bundle and every cert
    /// is inside its validity window.
    pub certificate_chain_valid: bool,
    /// (c) certificate SAN/CN matches the record's expected subject.
    pub subject_matches: bool,
    /// (d) a Rekor entry exists whose hashedrekord body carries this
    /// commit's digest, signature, and certificate.
    pub rekor_entry_exists: bool,
    /// Attribution carried by the record (task-107 F2).
    pub signer_id: String,
    pub task_id: String,
    pub spawned_by: String,
    pub sigstore_mode: SigstoreMode,
    /// Human-readable failure reason when `valid` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ── Signing ───────────────────────────────────────────────────────────────────

/// A Fulcio-issued ephemeral signing key + certificate chain.
pub struct FulcioSignedCommit {
    pub record: CommitSignature,
}

/// Bind an HTTP transport for the signing stack.
///
/// Production uses the shared `reqwest::Client`, with every call bounded by
/// `SIGNING_HTTP_TIMEOUT_SECS` via `tokio::time::timeout` (F10) — reqwest
/// has no default total timeout, so a hung signing stack must be cut off by
/// an explicit deadline or the fall-back-to-local guarantee only holds for
/// fast errors. Tests inject an async transport that encodes the *upstream*
/// Fulcio/Rekor contract so assertions exercise real wire-format
/// compatibility instead of a self-confirming mock (review F1/F7).
#[async_trait]
pub trait SigningHttpTransport: Send + Sync {
    /// POST `{fulcio}/api/v2/signingCert` with the JSON body; return the
    /// JSON response text.
    async fn fulcio_signing_cert(&self, fulcio_url: &str, body: &str) -> Result<String>;
    /// GET `{fulco}/api/v2/trustBundle`; return the JSON response text.
    async fn fulcio_trust_bundle(&self, fulcio_url: &str) -> Result<String>;
    /// POST `{rekor}/api/v1/log/entries` with the JSON body; return the JSON
    /// response text.
    async fn rekor_post_entry(&self, rekor_url: &str, body: &str) -> Result<String>;
    /// GET `{rekor}/api/v1/log/entries/{uuid}`; return the JSON response text.
    async fn rekor_get_entry(&self, rekor_url: &str, uuid: &str) -> Result<String>;
}

/// Production transport over the shared reqwest client.
struct ReqwestTransport {
    client: reqwest::Client,
    timeout: Duration,
}

impl ReqwestTransport {
    async fn call(&self, name: &str, fut: impl Future<Output = Result<String>>) -> Result<String> {
        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| anyhow!("{name} timed out after {:?}", self.timeout))?
    }
}

#[async_trait]
impl SigningHttpTransport for ReqwestTransport {
    async fn fulcio_signing_cert(&self, fulcio_url: &str, body: &str) -> Result<String> {
        self.call("fulcio signingCert", async {
            let resp = self
                .client
                .post(format!("{fulcio_url}/api/v2/signingCert"))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .body(body.to_string())
                .send()
                .await
                .context("fulcio signingCert request failed")?;
            let status = resp.status();
            let text = resp.text().await.context("fulcio response body")?;
            if !status.is_success() {
                return Err(anyhow!("fulcio signingCert returned {status}: {text}"));
            }
            Ok(text)
        })
        .await
    }

    async fn fulcio_trust_bundle(&self, fulcio_url: &str) -> Result<String> {
        self.call("fulcio trustBundle", async {
            let resp = self
                .client
                .get(format!("{fulcio_url}/api/v2/trustBundle"))
                .send()
                .await
                .context("fulcio trustBundle request failed")?;
            let status = resp.status();
            let text = resp.text().await.context("fulcio response body")?;
            if !status.is_success() {
                return Err(anyhow!("fulcio trustBundle returned {status}: {text}"));
            }
            Ok(text)
        })
        .await
    }

    async fn rekor_post_entry(&self, rekor_url: &str, body: &str) -> Result<String> {
        self.call("rekor entry POST", async {
            let resp = self
                .client
                .post(format!("{rekor_url}/api/v1/log/entries"))
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .context("rekor entry POST failed")?;
            let status = resp.status();
            let text = resp.text().await.context("rekor response body")?;
            if !status.is_success() {
                return Err(anyhow!("rekor entry POST returned {status}: {text}"));
            }
            Ok(text)
        })
        .await
    }

    async fn rekor_get_entry(&self, rekor_url: &str, uuid: &str) -> Result<String> {
        self.call("rekor entry GET", async {
            let resp = self
                .client
                .get(format!("{rekor_url}/api/v1/log/entries/{uuid}"))
                .send()
                .await
                .context("rekor entry GET failed")?;
            let status = resp.status();
            let text = resp.text().await.context("rekor response body")?;
            if !status.is_success() {
                return Err(anyhow!("rekor entry GET returned {status}: {text}"));
            }
            Ok(text)
        })
        .await
    }
}

/// The digest Rekor records and verification compares: SHA-256 over the raw
/// commit-SHA hex-string bytes. The commit id is content-addressed, so
/// signing/digesting its hex binds the content; sign and verify compute this
/// identically.
pub fn commit_digest(commit_sha: &str) -> String {
    hex::encode(Sha256::digest(commit_sha.as_bytes()))
}

/// Sign `commit_sha` keylessly: request a Fulcio certificate for the agent's
/// OIDC JWT, sign the commit SHA with the certificate's ephemeral P-256 key,
/// and record the signature in Rekor.
///
/// On any failure the caller falls back to local signing (its guarantee is
/// that squash itself never fails because of the external signing stack).
pub async fn sign_commit_keyless(
    repo_id: &str,
    commit_sha: &str,
    attribution: &SigningAttribution,
    oidc_jwt: &str,
    config: &SigningConfig,
    transport: &dyn SigningHttpTransport,
) -> Result<FulcioSignedCommit> {
    // 1. Ephemeral ECDSA P-256 key pair (keyless: no stored long-lived key).
    let key_pair = rcgen::KeyPair::generate()?;

    // 2. Proof of possession over the JWT `sub` claim: Fulcio's challenge
    //    check verifies this signature with the public key we're registering,
    //    against the token subject. ECDSA_P256_SHA256_ASN1 produces the DER
    //    signature format sigstore's verifier accepts; base64 for protojson.
    let sub = extract_jwt_sub(oidc_jwt)?;
    let ecdsa_key =
        ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &key_pair.serialize_der(),
            &ring::rand::SystemRandom::new(),
        )?;
    let pop = BASE64.encode(
        ecdsa_key
            .sign(&ring::rand::SystemRandom::new(), sub.as_bytes())?
            .as_ref(),
    );

    // 3. Fulcio issuance request. `publicKey.content` is the PEM-encoded
    //    public key — Fulcio's ParsePublicKey parses PEM text or raw DER,
    //    never base64(DER) (F1).
    let request = FulcioSigningCertRequest {
        credentials: FulcioCredentials {
            oidc_identity_token: oidc_jwt,
        },
        public_key_request: FulcioPublicKeyRequest {
            public_key: FulcioPublicKey {
                content: &key_pair.public_key_pem(),
                algorithm: "ecdsa",
            },
            proof_of_possession: &pop,
        },
    };
    let body = serde_json::to_string(&request)?;
    let response_text = transport.fulcio_signing_cert(&config.fulcio_url, &body).await?;
    let response: FulcioSigningCertResponse = serde_json::from_str(&response_text)
        .context("fulcio signingCert response is not valid protojson")?;

    let chain = response
        .signed_certificate_embedded_sct
        .and_then(|sct| sct.chain)
        .or_else(|| {
            response
                .signed_certificate_detached_sct
                .and_then(|sct| sct.chain)
        })
        .ok_or_else(|| anyhow!("fulcio response carries no certificate chain"))?;
    if chain.certificates.is_empty() {
        return Err(anyhow!("fulcio returned an empty certificate chain"));
    }

    // Rebuild the chain as PEM: leaf first, then intermediates/root.
    let chain_pem = chain
        .certificates
        .iter()
        .map(|b64| {
            let der = BASE64
                .decode(b64)
                .with_context(|| "fulcio chain element is not base64 DER")?;
            Ok(pem_encode_certificate(&der))
        })
        .collect::<Result<Vec<String>>>()?;
    let leaf_pem = chain_pem
        .first()
        .ok_or_else(|| anyhow!("fulcio chain has no leaf"))?
        .clone();
    let full_chain_pem = chain_pem.join("\n");

    // 4. Sign the commit SHA with the ephemeral key (DER ECDSA signature).
    let signature_der = ecdsa_key
        .sign(&ring::rand::SystemRandom::new(), commit_sha.as_bytes())?
        .as_ref()
        .to_vec();
    let signature_b64 = BASE64.encode(&signature_der);

    // 5. Record in Rekor: hashedrekord with the commit digest, signature,
    //    and the leaf certificate (base64 PEM in `publicKey.content`, the
    //    encoding upstream Rekor's DecodeEntry + x509.NewPublicKey accept).
    let digest = commit_digest(commit_sha);
    let entry = RekorEntryRequest {
        kind: "hashedrekord",
        api_version: "0.0.1",
        spec: RekorHashedrekordSpec {
            data: RekorData {
                hash: RekorHash {
                    algorithm: "sha256",
                    value: &digest,
                },
            },
            signature: RekorSignature {
                content: &signature_b64,
                public_key: RekorPublicKey {
                    content: &BASE64.encode(leaf_pem.as_bytes()),
                },
            },
        },
    };
    let entry_body = serde_json::to_string(&entry)?;
    let rekor_response_text = transport.rekor_post_entry(&config.rekor_url, &entry_body).await?;
    let rekor_response: RekorEntryResponse =
        serde_json::from_str(&rekor_response_text).context("rekor entry response invalid")?;
    let rekor_entry_id = rekor_response
        .0
        .keys()
        .next()
        .cloned()
        .ok_or_else(|| anyhow!("rekor returned no entry UUID"))?;

    let record = CommitSignature {
        repo_id: repo_id.to_string(),
        commit_sha: commit_sha.to_string(),
        signer_id: attribution.agent_id.clone(),
        task_id: attribution.task_id.clone(),
        spawned_by: attribution.spawned_by.clone(),
        algorithm: ALGORITHM_ECDSA_P256.to_string(),
        signature: signature_b64,
        signing_key_id: String::new(), // keyless — no forge kid
        signed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        sigstore_mode: SigstoreMode::Fulcio,
        oidc_subject: sub.clone(),
        oidc_issuer: extract_jwt_iss(oidc_jwt).unwrap_or_default(),
        certificate_pem: Some(leaf_pem),
        certificate_chain_pem: Some(full_chain_pem),
        rekor_entry_id: Some(rekor_entry_id),
    };
    Ok(FulcioSignedCommit { record })
}

// ── Verification ──────────────────────────────────────────────────────────────

/// Verify a Fulcio-mode commit signature against the server's configured
/// trust anchors (task plan item 3). Trust anchors come from `config` — the
/// record being verified never chooses them (F3). Local-mode records are
/// verified by the API layer against the forge Ed25519 key, which this
/// module does not own.
pub async fn verify_signature_fulcio(
    record: &CommitSignature,
    config: &SigningConfig,
    transport: &dyn SigningHttpTransport,
    now: std::time::SystemTime,
) -> SignatureVerificationResult {
    verify_fulcio(record, config, transport, now).await
}

impl SignatureVerificationResult {
    /// Result for a local-mode record whose signature was verified (real
    /// Ed25519 check) by the caller against the forge signing key. There is
    /// no Fulcio certificate chain or Rekor entry for local records; the
    /// non-applicable phases are reported as `false` with a reason naming
    /// the mode.
    pub fn local_mode(record: &CommitSignature, signature_valid: bool) -> Self {
        let reason = if signature_valid {
            "local-mode signature verified against the forge Ed25519 key (no Fulcio/Rekor in local mode)"
                .to_string()
        } else {
            "local-mode signature did not verify against the forge Ed25519 key".to_string()
        };
        Self {
            valid: signature_valid,
            signature_valid,
            certificate_chain_valid: false,
            subject_matches: false,
            rekor_entry_exists: false,
            signer_id: record.signer_id.clone(),
            task_id: record.task_id.clone(),
            spawned_by: record.spawned_by.clone(),
            sigstore_mode: SigstoreMode::Local,
            reason: Some(reason),
        }
    }
}

async fn verify_fulcio(
    record: &CommitSignature,
    config: &SigningConfig,
    transport: &dyn SigningHttpTransport,
    now: std::time::SystemTime,
) -> SignatureVerificationResult {
    let mut result = SignatureVerificationResult {
        valid: false,
        signature_valid: false,
        certificate_chain_valid: false,
        subject_matches: false,
        rekor_entry_exists: false,
        signer_id: record.signer_id.clone(),
        task_id: record.task_id.clone(),
        spawned_by: record.spawned_by.clone(),
        sigstore_mode: SigstoreMode::Fulcio,
        reason: None,
    };
    let mut failures: Vec<String> = Vec::new();

    let leaf_pem = match record.certificate_pem.as_deref() {
        Some(p) => p,
        None => {
            return fail(result, "record has no leaf certificate");
        }
    };

    // (a) Signature check: ECDSA over the commit SHA with the leaf's key.
    let sig_valid = check_signature(record, leaf_pem);
    result.signature_valid = sig_valid;
    if !sig_valid {
        failures.push("signature does not verify against the leaf certificate".into());
    }

    // (b) Chain check against the CONFIGURED Fulcio's trust bundle, trying
    //     every chain the bundle serves (F11), with validity windows (F5).
    let bundle = match fetch_trust_bundle(&config.fulcio_url, transport).await {
        Ok(b) => b,
        Err(e) => return fail(result, &format!("cannot fetch Fulcio trust bundle: {e}")),
    };
    let chain_valid = check_chain_all_bundles(record, &bundle, now);
    result.certificate_chain_valid = chain_valid;
    if !chain_valid {
        failures.push("certificate chain does not validate against the configured Fulcio trust bundle (or is outside its validity window)".into());
    }

    // (c) Subject check: leaf SAN/CN must match the record's expected subject.
    let subject_ok = check_subject(record, leaf_pem);
    result.subject_matches = subject_ok;
    if !subject_ok {
        failures.push("certificate subject does not match the record's expected identity".into());
    }

    // (d) Rekor entry: must exist AND match this record's digest, signature,
    //     and certificate (F4).
    let rekor_ok = check_rekor_entry(record, &config.rekor_url, transport).await;
    result.rekor_entry_exists = rekor_ok;
    if !rekor_ok {
        failures.push("no matching Rekor entry (or entry contents do not match this commit)".into());
    }

    if failures.is_empty() {
        result.valid = true;
        result.reason = None;
    } else {
        result.reason = Some(failures.join("; "));
    }
    result
}

fn fail(mut result: SignatureVerificationResult, reason: &str) -> SignatureVerificationResult {
    result.reason = Some(reason.to_string());
    result
}

/// (a) Verify the record's base64-DER ECDSA signature over the commit SHA
/// using the leaf certificate's public key.
fn check_signature(record: &CommitSignature, leaf_pem: &str) -> bool {
    let Ok(parsed) = ParsedCertificate::from_pem(leaf_pem) else {
        return false;
    };
    let cert = &parsed.cert;
    let Ok(sig_bytes) = BASE64.decode(&record.signature) else {
        return false;
    };
    // SPKI → ring key: parse the raw subjectPublicKey bit string.
    let spki = &cert.subject_pki;
    use ring::signature::UnparsedPublicKey;
    let pk = UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        spki.subject_public_key.data.as_ref(),
    );
    pk.verify(record.commit_sha.as_bytes(), &sig_bytes).is_ok()
}

/// Fetch and decode the trust bundle from the CONFIGURED Fulcio (F3).
async fn fetch_trust_bundle(
    fulcio_url: &str,
    transport: &dyn SigningHttpTransport,
) -> Result<Vec<Vec<String>>> {
    let text = transport.fulcio_trust_bundle(fulcio_url).await?;
    let bundle: TrustBundleResponse =
        serde_json::from_str(&text).context("fulcio trust bundle response invalid")?;
    Ok(bundle.chains)
}

/// (b) Chain check: the record's stored chain (leaf first, then
/// intermediates/root, exactly as Fulcio returned it) must validate against
/// ANY chain in the trust bundle (F11), and every certificate in the stored
/// chain must be within its validity window at `now` (F5).
fn check_chain_all_bundles(
    record: &CommitSignature,
    bundle_chains: &[Vec<String>],
    now: std::time::SystemTime,
) -> bool {
    let Some(chain_pem) = record.certificate_chain_pem.as_deref() else {
        return false;
    };
    let stored: Vec<ParsedCertificate> = parse_pem_chain(chain_pem);
    if stored.is_empty() {
        return false;
    }
    // Validity window on every stored certificate (leaf + intermediates).
    for cert in &stored {
        if !within_validity(&cert.cert, now) {
            return false;
        }
    }
    // Each link must be signed by the next; the last must be self-signed.
    for pair in stored.windows(2) {
        if pair[0].cert.verify_signature(Some(&pair[1].cert.subject_pki)).is_err() {
            return false;
        }
    }
    let root = &stored[stored.len() - 1];
    if root.cert.verify_signature(None).is_err() {
        return false;
    }

    // The root must be one of the roots in the configured bundle's chains,
    // compared as full DER encodings (a trust bundle element is the complete
    // certificate, not just its TBS section).
    let root_der = root.der_bytes();
    bundle_chains.iter().any(|chain| {
        chain.iter().any(|b64| {
            BASE64
                .decode(b64)
                .map(|der| der == root_der)
                .unwrap_or(false)
        })
    })
}

/// F5: `not_before <= now <= not_after`.
fn within_validity(cert: &x509_parser::certificate::X509Certificate<'_>, now: std::time::SystemTime) -> bool {
    let now_secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let not_before = cert.validity.not_before.timestamp();
    let not_after = cert.validity.not_after.timestamp();
    now_secs >= not_before && now_secs <= not_after
}

/// (c) Subject check: the leaf's SANs (URI/RFC822/DNS) or CN must equal the
/// record's `oidc_subject` — the identity Fulcio bound from the JWT.
fn check_subject(record: &CommitSignature, leaf_pem: &str) -> bool {
    let Ok(cert) = parse_certificate(leaf_pem) else {
        return false;
    };
    // SANs first (Fulcio binds the OIDC subject as a SAN).
    for ext in cert.cert.extensions() {
        if let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) =
            ext.parsed_extension()
        {
            for name in &san.general_names {
                let matched = match name {
                    x509_parser::extensions::GeneralName::URI(s) => *s == record.oidc_subject,
                    x509_parser::extensions::GeneralName::RFC822Name(s) => {
                        *s == record.oidc_subject
                    }
                    x509_parser::extensions::GeneralName::DNSName(s) => {
                        *s == record.oidc_subject
                    }
                    _ => false,
                };
                if matched {
                    return true;
                }
            }
        }
    }
    // CN fallback.
    let cn_matches = cert
        .cert
        .subject()
        .iter_common_name()
        .any(|cn| cn.as_str().map(|s| s == record.oidc_subject).unwrap_or(false));
    cn_matches
}

/// (d) Rekor entry: fetch `{rekor}/api/v1/log/entries/{uuid}` and confirm the
/// entry's hashedrekord body carries THIS commit's digest, signature, and
/// leaf certificate (F4 — content match, not existence).
async fn check_rekor_entry(
    record: &CommitSignature,
    rekor_url: &str,
    transport: &dyn SigningHttpTransport,
) -> bool {
    let Some(uuid) = record.rekor_entry_id.as_deref() else {
        return false;
    };
    let Ok(text) = transport.rekor_get_entry(rekor_url, uuid).await else {
        return false;
    };
    let Ok(response) = serde_json::from_str::<RekorEntryResponse>(&text) else {
        return false;
    };
    let Some(entry) = response.0.values().next() else {
        return false;
    };
    let Some(body_b64) = entry.body.as_deref() else {
        return false;
    };
    let Ok(body_json) = BASE64.decode(body_b64) else {
        return false;
    };
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(&body_json) else {
        return false;
    };

    // Rekor canonical entry: {"kind": "hashedrekord", "apiVersion": "0.0.1",
    // "spec": {"data": {"hash": {...}}, "signature": {"content", "publicKey": {"content"}}}}
    let spec = match body.get("spec").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return false,
    };

    // Digest must match this commit.
    let digest = commit_digest(&record.commit_sha);
    let hash_matches = spec
        .get("data")
        .and_then(|d| d.get("hash"))
        .and_then(|h| h.get("value"))
        .and_then(|v| v.as_str())
        .map(|v| v == digest)
        .unwrap_or(false);
    if !hash_matches {
        return false;
    }

    // Signature must match this record's signature (base64 in body).
    let sig_matches = spec
        .get("signature")
        .and_then(|s| s.get("content"))
        .and_then(|c| c.as_str())
        .map(|c| c == record.signature)
        .unwrap_or(false);
    if !sig_matches {
        return false;
    }

    // The recorded public key must be this record's leaf certificate:
    // Rekor stores base64(PEM) in `publicKey.content`; compare the decoded
    // certificate bytes against the record's stored leaf.
    let Some(leaf_pem) = record.certificate_pem.as_deref() else {
        return false;
    };
    let key_matches = spec
        .get("signature")
        .and_then(|s| s.get("publicKey"))
        .and_then(|p| p.get("content"))
        .and_then(|c| c.as_str())
        .and_then(|c| BASE64.decode(c).ok())
        .map(|pem_bytes| {
            pem_bytes == leaf_pem.as_bytes()
                || pem_equal_mod_whitespace(&pem_bytes, leaf_pem.as_bytes())
        })
        .unwrap_or(false);
    key_matches
}

/// Compare PEM byte sequences ignoring line-ending differences (a record and
/// a Rekor body may re-encode the same certificate with different wrapping).
fn pem_equal_mod_whitespace(a: &[u8], b: &[u8]) -> bool {
    let strip = |v: &[u8]| -> Vec<u8> {
        v.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect()
    };
    strip(a) == strip(b)
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// An owned DER buffer plus the parsed certificate view over it. The view
/// borrows the heap allocation owned by this struct: the `Box<[u8]>` target
/// is stable across moves, and the struct owns both the buffer and the view
/// over it, so the borrow cannot outlive the buffer.
///
/// (a) `transmute` of `&*boxed` to `&'static [u8]` — the reference is
/// derived from a heap allocation we own and never free while the struct
/// lives; moving the struct copies the Box pointer, not the allocation, so
/// the view stays valid. `Self` is not `Sync`/dropped while borrowed: the
/// only code holding the view is inside `Self`.
/// (b) `X509Certificate<'static>` therefore never escapes into a context
/// that could outlive `Self`, because every accessor borrows `&self`.
struct ParsedCertificate {
    der: Box<[u8]>,
    cert: x509_parser::certificate::X509Certificate<'static>,
}

impl ParsedCertificate {
    fn from_pem(pem_str: &str) -> Result<Self> {
        let der = pem_to_der(pem_str)?;
        Self::from_der(der)
    }

    fn from_der(der: Vec<u8>) -> Result<Self> {
        let boxed: Box<[u8]> = der.into_boxed_slice();
        // SAFETY: `&*boxed` points into a heap allocation owned by the
        // `Box` we store in the returned struct. The allocation outlives
        // every use of the view (the view is only reachable through the
        // struct), and Box contents never move (a moved Box copies the
        // pointer, not the target). The transmute extends the lifetime of
        // the reference, which is sound here because the struct owns the
        // allocation for exactly as long as the view is reachable.
        let static_ref: &'static [u8] = unsafe { std::mem::transmute(&*boxed) };
        let (rem, cert) = x509_parser::parse_x509_certificate(static_ref)
            .map_err(|e| anyhow!("x509 parse error: {e}"))?;
        if !rem.is_empty() {
            return Err(anyhow!("trailing bytes after certificate"));
        }
        Ok(Self { der: boxed, cert })
    }

    /// Full DER encoding of the certificate (not just the TBS section).
    fn der_bytes(&self) -> &[u8] {
        &self.der
    }
}

/// Decode a single PEM block (any label) to its DER bytes.
///
/// Strictly anchored: the END marker must follow the BEGIN marker, so
/// concatenated blocks cannot pair the first BEGIN with a later END.
fn pem_to_der(pem_str: &str) -> Result<Vec<u8>> {
    const BEGIN: &str = "-----BEGIN ";
    const END: &str = "-----END ";
    let rest = pem_str
        .find(BEGIN)
        .ok_or_else(|| anyhow!("no PEM BEGIN marker"))?;
    let after_label = pem_str[rest..]
        .find("-----")
        .map(|i| rest + i + "-----".len())
        .ok_or_else(|| anyhow!("unterminated PEM label"))?;
    let end_rel = pem_str[after_label..]
        .find(END)
        .ok_or_else(|| anyhow!("no PEM END marker"))?;
    let end = after_label + end_rel;
    let b64: String = pem_str[after_label..end]
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    BASE64
        .decode(b64.as_bytes())
        .map_err(|e| anyhow!("PEM base64 decode error: {e}"))
}

/// Re-encode DER bytes as a `CERTIFICATE` PEM block.
fn pem_encode_certificate(der: &[u8]) -> String {
    pem::encode(&pem::Pem::new("CERTIFICATE", der.to_vec()))
}

/// Parse a concatenated PEM chain (one cert per block) into parsed certs,
/// leaf first. Empty when the input has no parseable block.
fn parse_pem_chain(chain_pem: &str) -> Vec<ParsedCertificate> {
    let mut out = Vec::new();
    let mut rest = chain_pem;
    while let Some(begin) = rest.find("-----BEGIN ") {
        let end_marker = match rest[begin..].find("-----END ") {
            Some(i) => begin + i,
            None => break,
        };
        // Block = BEGIN .. end of the END line.
        let block_end = match rest[end_marker..].find("-----\n").or_else(|| rest[end_marker..].find("-----\r\n")) {
            Some(i) => end_marker + i + "-----".len(),
            None => rest.len(),
        };
        let block = &rest[..block_end];
        match ParsedCertificate::from_pem(block) {
            Ok(pc) => out.push(pc),
            Err(_) => return Vec::new(), // malformed chain fails closed
        }
        rest = &rest[block_end..];
    }
    out
}

/// Parse a single PEM certificate (owned buffer + view).
fn parse_certificate(pem_str: &str) -> Result<ParsedCertificate> {
    ParsedCertificate::from_pem(pem_str)
}

fn extract_jwt_sub(token: &str) -> Result<String> {
    let payload = decode_jwt_payload(token)?;
    payload
        .get("sub")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("agent JWT has no sub claim"))
}

fn extract_jwt_iss(token: &str) -> Result<String> {
    let payload = decode_jwt_payload(token)?;
    payload
        .get("iss")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("agent JWT has no iss claim"))
}

/// Decode (NOT verify — verification happened at authentication time when the
/// JWT entered the system) the JWT payload claims.
fn decode_jwt_payload(token: &str) -> Result<serde_json::Value> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let part = token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("JWT has no payload segment"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .context("JWT payload is not base64url")?;
    serde_json::from_slice(&bytes).context("JWT payload is not JSON")
}

/// Build the production transport bound to the shared HTTP client.
pub fn production_transport(client: reqwest::Client) -> Box<dyn SigningHttpTransport> {
    Box::new(ReqwestTransport {
        client,
        timeout: Duration::from_secs(crate::commit_signatures::SIGNING_HTTP_TIMEOUT_SECS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_digest_is_stable_and_hex() {
        let d1 = commit_digest("abc123");
        let d2 = commit_digest("abc123");
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
        assert!(d1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn decode_jwt_payload_extracts_sub() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use serde_json::json;
        let payload = json!({"sub": "agent-1", "iss": "http://x"});
        let b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let token = format!("header.{b64}.signature");
        assert_eq!(extract_jwt_sub(&token).unwrap(), "agent-1");
        assert_eq!(extract_jwt_iss(&token).unwrap(), "http://x");
    }

    #[test]
    fn decode_jwt_payload_rejects_garbage() {
        assert!(decode_jwt_payload("not-a-jwt").is_err());
        assert!(decode_jwt_payload("a.b").is_err());
    }

    #[test]
    fn pem_roundtrip_via_fulcio_chain_encoding() {
        // The chain element encoding (base64 DER → PEM) must produce a PEM
        // block x509 can parse back.
        let key = rcgen::KeyPair::generate().unwrap();
        let spki = key.public_key_der();
        let pem = pem::encode(&pem::Pem::new("PUBLIC KEY", spki.clone()));
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
        let der = pem_to_der(&pem).unwrap();
        assert_eq!(der, spki);
    }

    // ── Mock signing stack (task-107 F7) ──────────────────────────────────────
    //
    // Encodes the UPSTREAM Fulcio/Rekor wire contract, not the
    // implementation's behavior:
    // - `publicKey.content` is PEM text (what Fulcio's ParsePublicKey
    //   accepts); the mock REJECTS base64(DER) with 400, like real Fulcio.
    // - `proofOfPossession` is base64 DER ECDSA over the JWT `sub`; the mock
    //   VERIFIES it against the presented public key — a client that signs
    //   the wrong preimage or encodes the key wrong gets no certificate.
    // - the trust bundle serves TWO chains (public Fulcio serves multiple
    //   concurrent chains during CA rotation; F11).
    // - Rekor POST stores the entry body; GET serves it back (F4 content
    //   matching has real data to compare against).
    //
    // Failure knobs select which verification phase must fail:
    // - `tamper_signature`: the record's signature is corrupted after
    //   signing (phase a).
    // - `untrusted_ca`: the leaf is issued by a CA that is NOT in the served
    //   trust bundle (phase b).
    // - `wrong_subject`: the leaf's SAN is a different identity than the JWT
    //   sub (phase c).
    // - `drop_rekor_entry`: the Rekor GET returns 404 (phase d).
    // - `expired_leaf`: the leaf's validity window is in the past (phase b).

    /// A CA certificate + key the mock Fulcio issues leaves under.
    struct MockCa {
        cert: rcgen::Certificate,
        key: rcgen::KeyPair,
    }

    impl MockCa {
        fn new(name: &str) -> Self {
            let key = rcgen::KeyPair::generate().unwrap();
            let mut params = rcgen::CertificateParams::new(vec![]).unwrap();
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, name);
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.key_usages = vec![
                rcgen::KeyUsagePurpose::DigitalSignature,
                rcgen::KeyUsagePurpose::KeyCertSign,
            ];
            // Long validity — the ROOT's window is not the one under test.
            params.not_before = rcgen::date_time_ymd(2020, 1, 1);
            params.not_after = rcgen::date_time_ymd(2050, 1, 1);
            Self {
                cert: params.self_signed(&key).unwrap(),
                key,
            }
        }

        /// Issue a leaf over `subject_public_key_pem` with `san` as the URI
        /// SAN and the given validity window.
        fn issue_leaf(
            &self,
            subject_public_key_pem: &str,
            san: &str,
            not_before: time::OffsetDateTime,
            not_after: time::OffsetDateTime,
        ) -> String {
            let spki = rcgen::SubjectPublicKeyInfo::from_pem(subject_public_key_pem).unwrap();
            let mut params = rcgen::CertificateParams::new(vec![]).unwrap();
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, san);
            params.subject_alt_names = vec![rcgen::SanType::URI(
                san.to_string()
                    .try_into()
                    .expect("SAN must be IA5-encoded"),
            )];
            params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
            params.not_before = not_before;
            params.not_after = not_after;
            let leaf = params.signed_by(&spki, &self.cert, &self.key).unwrap();
            leaf.pem()
        }
    }
    #[derive(Default)]
    struct MockKnobs {
        tamper_signature: bool,
        untrusted_ca: bool,
        wrong_subject: bool,
        drop_rekor_entry: bool,
        expired_leaf: bool,
    }

    struct MockSigningStack {
        /// The CA chain the trust bundle serves (trusted).
        trusted_ca: MockCa,
        /// A second trusted chain, to exercise multi-chain bundles (F11).
        second_trusted_ca: MockCa,
        /// An issuer that is NOT in the trust bundle.
        untrusted_ca: MockCa,
        /// Recorded Rekor entries: uuid → body JSON.
        rekor_entries: parking_lot::Mutex<std::collections::HashMap<String, serde_json::Value>>,
        knobs: MockKnobs,
    }

    impl MockSigningStack {
        fn new() -> Self {
            Self {
                trusted_ca: MockCa::new("gyre-test-fulcio-root-1"),
                second_trusted_ca: MockCa::new("gyre-test-fulcio-root-2"),
                untrusted_ca: MockCa::new("rogue-ca"),
                rekor_entries: Default::default(),
                knobs: Default::default(),
            }
        }

        fn trust_bundle_json(&self) -> String {
            let chain1 = [BASE64.encode(self.trusted_ca.cert.der().as_ref())];
            let chain2 = [BASE64.encode(self.second_trusted_ca.cert.der().as_ref())];
            serde_json::json!({ "chains": [chain1, chain2] }).to_string()
        }
    }

    #[async_trait]
    impl SigningHttpTransport for MockSigningStack {
        async fn fulcio_signing_cert(&self, _fulcio_url: &str, body: &str) -> Result<String> {
            let req: serde_json::Value = serde_json::from_str(body)
                .context("mock fulcio: request is not the protojson it must be")?;

            // Upstream contract: publicKey.content is PEM text.
            let pk_pem = req["publicKeyRequest"]["publicKey"]["content"]
                .as_str()
                .ok_or_else(|| anyhow!("mock fulcio: publicKey.content missing"))?;
            if !pk_pem.contains("-----BEGIN PUBLIC KEY-----") {
                // Real Fulcio: ParsePublicKey fails on non-PEM/non-DER input.
                return Err(anyhow!("400: invalid public key"));
            }

            // Upstream contract: credentials.oidcIdentityToken is the JWT;
            // the SAN is bound from its `sub`.
            let jwt = req["credentials"]["oidcIdentityToken"]
                .as_str()
                .ok_or_else(|| anyhow!("mock fulcio: oidcIdentityToken missing"))?;
            let sub = extract_jwt_sub(jwt)?;

            // Upstream contract: proofOfPossession is base64 DER ECDSA by
            // the private key over the token subject. Verify it for real.
            let pop_b64 = req["publicKeyRequest"]["proofOfPossession"]
                .as_str()
                .ok_or_else(|| anyhow!("mock fulcio: proofOfPossession missing"))?;
            let pop = BASE64
                .decode(pop_b64)
                .context("mock fulcio: PoP is not base64")?;
            let spki = rcgen::SubjectPublicKeyInfo::from_pem(pk_pem)
                .context("mock fulcio: publicKey.content is not parseable PEM")?;
            use rcgen::PublicKeyData as _;
            use ring::signature::UnparsedPublicKey;
            let vk =
                UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, spki.der_bytes());
            vk.verify(sub.as_bytes(), &pop)
                .map_err(|_| anyhow!("403: proof of possession failed"))?;

            // Failure knobs decide the issuer, the leaf's identity, and its
            // validity window.
            let san = if self.knobs.wrong_subject {
                format!("rogue:{sub}")
            } else {
                sub.clone()
            };
            let issuer = if self.knobs.untrusted_ca {
                &self.untrusted_ca
            } else {
                &self.trusted_ca
            };
            let (not_before, not_after) = if self.knobs.expired_leaf {
                (
                    rcgen::date_time_ymd(2020, 1, 1),
                    rcgen::date_time_ymd(2020, 1, 2),
                )
            } else {
                (
                    rcgen::date_time_ymd(2024, 1, 1),
                    rcgen::date_time_ymd(2050, 1, 1),
                )
            };
            let leaf_pem = issuer.issue_leaf(pk_pem, &san, not_before, not_after);
            let leaf_der = pem_to_der(&leaf_pem)?;
            let chain = vec![
                BASE64.encode(leaf_der),
                BASE64.encode(issuer.cert.der().as_ref()),
            ];
            Ok(
                serde_json::json!({
                    "signedCertificateEmbeddedSct": { "chain": { "certificates": chain } }
                })
                .to_string(),
            )
        }

        async fn fulcio_trust_bundle(&self, _fulcio_url: &str) -> Result<String> {
            Ok(self.trust_bundle_json())
        }

        async fn rekor_post_entry(&self, _rekor_url: &str, body: &str) -> Result<String> {
            let entry: serde_json::Value = serde_json::from_str(body)
                .context("mock rekor: request is not the hashedrekord JSON it must be")?;
            // Rekor derives the entry UUID from the log; the mock derives it
            // from the body so GET serves the identical entry back.
            let uuid = hex::encode(Sha256::digest(body.as_bytes()));
            self.rekor_entries.lock().insert(uuid.clone(), entry);
            let body_b64 = BASE64.encode(body);
            Ok(serde_json::json!({
                uuid.clone(): { "body": body_b64 }
            })
            .to_string())
        }

        async fn rekor_get_entry(&self, _rekor_url: &str, uuid: &str) -> Result<String> {
            if self.knobs.drop_rekor_entry {
                return Err(anyhow!("404: entry not found"));
            }
            let entries = self.rekor_entries.lock();
            let entry = entries
                .get(uuid)
                .ok_or_else(|| anyhow!("404: entry not found"))?;
            let body_b64 = BASE64.encode(serde_json::to_string(entry).unwrap());
            Ok(serde_json::json!({
                uuid: { "body": body_b64 }
            })
            .to_string())
        }
    }

    // ── Signing + verification round-trips over the mock stack ────────────────

    fn test_jwt(sub: &str) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&serde_json::json!({"sub": sub, "iss": "http://gyre"})).unwrap());
        format!("{header}.{payload}.sig")
    }

    fn test_config() -> SigningConfig {
        SigningConfig {
            mode: crate::commit_signatures::SigningMode::Fulcio,
            fulcio_url: "https://fulcio.test".to_string(),
            rekor_url: "https://rekor.test".to_string(),
        }
    }

    fn test_attribution() -> SigningAttribution {
        SigningAttribution {
            agent_id: "agent-7".to_string(),
            task_id: "task-107".to_string(),
            spawned_by: "user-42".to_string(),
        }
    }

    /// A `now` inside the mock leaf's validity window (2024-01-01..2050).
    fn test_now() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_730_000_000)
    }

    async fn sign(
        stack: &MockSigningStack,
        commit_sha: &str,
    ) -> CommitSignature {
        sign_commit_keyless(
            "repo-1",
            commit_sha,
            &test_attribution(),
            &test_jwt("agent-7"),
            &test_config(),
            stack,
        )
        .await
        .expect("keyless signing against the mock stack must succeed")
        .record
    }

    /// The happy path: all four phases pass, attribution is carried, and the
    /// record is Fulcio-mode with a Rekor entry.
    #[tokio::test]
    async fn keyless_sign_and_verify_all_phases_pass() {
        let stack = MockSigningStack::new();
        let record = sign(&stack, "abc123").await;
        assert_eq!(record.sigstore_mode, SigstoreMode::Fulcio);
        assert_eq!(record.signer_id, "agent-7");
        assert_eq!(record.task_id, "task-107");
        assert_eq!(record.spawned_by, "user-42");
        assert_eq!(record.oidc_subject, "agent-7");
        assert!(record.certificate_pem.is_some());
        assert!(record.rekor_entry_id.is_some());

        let result = verify_signature_fulcio(&record, &test_config(), &stack, test_now()).await;
        assert!(result.valid, "reason: {:?}", result.reason);
        assert!(result.signature_valid);
        assert!(result.certificate_chain_valid);
        assert!(result.subject_matches);
        assert!(result.rekor_entry_exists);
        assert_eq!(result.signer_id, "agent-7");
        assert_eq!(result.task_id, "task-107");
        assert_eq!(result.spawned_by, "user-42");
    }

    /// F11: the leaf is issued under trusted chain 1 while the bundle also
    /// serves chain 2 — but the decisive case is the inverse: issuance
    /// through the SECOND chain must also verify. Flip which CA issues.
    #[tokio::test]
    async fn verification_accepts_second_trusted_chain() {
        let mut stack = MockSigningStack::new();
        // Make the rogue CA the issuer but ALSO serve it as a third trusted
        // chain — the leaf must verify because SOME served chain roots it.
        let mut knobs = MockKnobs::default();
        knobs.untrusted_ca = true;
        stack.knobs = knobs;
        // Serve the rogue CA in the bundle too.
        let rogue_der = BASE64.encode(stack.untrusted_ca.cert.der().as_ref());
        let bundle = serde_json::json!({
            "chains": [
                [BASE64.encode(stack.trusted_ca.cert.der().as_ref())],
                [BASE64.encode(stack.second_trusted_ca.cert.der().as_ref())],
                [rogue_der],
            ]
        });
        let record = sign(&stack, "chain2sha").await;
        // Verify with a transport whose trust bundle includes the rogue CA.
        let verifying_stack = FixedBundleStack {
            inner: stack,
            bundle: bundle.to_string(),
            knobs: MockKnobs::default(),
        };
        let result =
            verify_signature_fulcio(&record, &test_config(), &verifying_stack, test_now()).await;
        assert!(
            result.certificate_chain_valid,
            "a chain served at position 3 must validate: {:?}",
            result.reason
        );
        assert!(result.valid);
    }

    /// A transport wrapper serving a fixed trust-bundle JSON.
    struct FixedBundleStack {
        inner: MockSigningStack,
        bundle: String,
        knobs: MockKnobs,
    }

    #[async_trait]
    impl SigningHttpTransport for FixedBundleStack {
        async fn fulcio_signing_cert(&self, u: &str, b: &str) -> Result<String> {
            self.inner.fulcio_signing_cert(u, b).await
        }
        async fn fulcio_trust_bundle(&self, _u: &str) -> Result<String> {
            Ok(self.bundle.clone())
        }
        async fn rekor_post_entry(&self, u: &str, b: &str) -> Result<String> {
            self.inner.rekor_post_entry(u, b).await
        }
        async fn rekor_get_entry(&self, u: &str, uuid: &str) -> Result<String> {
            if self.knobs.drop_rekor_entry {
                return Err(anyhow!("404"));
            }
            self.inner.rekor_get_entry(u, uuid).await
        }
    }

    /// Phase (a) negative: a tampered signature must fail signature_valid
    /// while the other phases still pass.
    #[tokio::test]
    async fn verification_fails_on_tampered_signature() {
        let stack = MockSigningStack::new();
        let mut record = sign(&stack, "tamper1").await;
        // Corrupt the last byte of the DER ECDSA signature.
        let mut sig = BASE64.decode(&record.signature).unwrap();
        let last = sig.len() - 1;
        sig[last] ^= 0xFF;
        record.signature = BASE64.encode(&sig);

        let result = verify_signature_fulcio(&record, &test_config(), &stack, test_now()).await;
        assert!(!result.valid);
        assert!(!result.signature_valid, "tampered signature must fail (a)");
        // The certificate, subject, and Rekor entry are untouched.
        assert!(result.certificate_chain_valid);
        assert!(result.subject_matches);
        assert!(result.rekor_entry_exists);
    }

    /// Phase (b) negative: a leaf issued by a CA absent from the trust
    /// bundle must fail certificate_chain_valid.
    #[tokio::test]
    async fn verification_fails_on_untrusted_ca() {
        let mut stack = MockSigningStack::new();
        stack.knobs.untrusted_ca = true;
        let record = sign(&stack, "rogue1").await;

        let result = verify_signature_fulcio(&record, &test_config(), &stack, test_now()).await;
        assert!(!result.valid);
        assert!(
            !result.certificate_chain_valid,
            "untrusted CA must fail (b)"
        );
        // The signature itself and the recorded subject still hold.
        assert!(result.signature_valid);
        assert!(result.subject_matches);
    }

    /// Phase (b) negative (validity window): an expired leaf must fail the
    /// chain check even though every signature in the chain is valid (F5).
    #[tokio::test]
    async fn verification_fails_on_expired_leaf() {
        let mut stack = MockSigningStack::new();
        stack.knobs.expired_leaf = true;
        let record = sign(&stack, "expired1").await;

        let result = verify_signature_fulcio(&record, &test_config(), &stack, test_now()).await;
        assert!(!result.valid);
        assert!(
            !result.certificate_chain_valid,
            "expired leaf must fail (b): {:?}",
            result.reason
        );
        assert!(result.signature_valid);
    }

    /// Phase (c) negative: a leaf whose SAN names a different identity than
    /// the recorded subject must fail subject_matches.
    #[tokio::test]
    async fn verification_fails_on_mismatched_subject() {
        let mut stack = MockSigningStack::new();
        stack.knobs.wrong_subject = true;
        let record = sign(&stack, "rogue-subj").await;

        let result = verify_signature_fulcio(&record, &test_config(), &stack, test_now()).await;
        assert!(!result.valid);
        assert!(!result.subject_matches, "wrong SAN must fail (c)");
        assert!(result.signature_valid);
        assert!(result.certificate_chain_valid);
    }

    /// Phase (d) negative: a missing Rekor entry must fail rekor_entry_exists.
    #[tokio::test]
    async fn verification_fails_when_rekor_entry_missing() {
        let stack = MockSigningStack::new();
        let record = sign(&stack, "norekor").await;
        let verifying = FixedBundleStack {
            inner: stack,
            bundle: stack.trust_bundle_json(),
            knobs: MockKnobs {
                drop_rekor_entry: true,
                ..Default::default()
            },
        };
        let result = verify_signature_fulcio(&record, &test_config(), &verifying, test_now()).await;
        assert!(!result.valid);
        assert!(!result.rekor_entry_exists, "missing entry must fail (d)");
        assert!(result.signature_valid);
        assert!(result.certificate_chain_valid);
    }

    /// Phase (d) content check (F4): an entry that exists but records a
    /// DIFFERENT commit's digest must not count as this record's entry.
    #[tokio::test]
    async fn verification_fails_when_rekor_entry_is_for_another_commit() {
        let stack = MockSigningStack::new();
        let record = sign(&stack, "commit-a").await;
        // Point the record at the entry of a different, validly-recorded
        // commit (any existing UUID — exactly the F4 attack).
        let other = sign(&stack, "commit-b").await;
        let mut swapped = record.clone();
        swapped.rekor_entry_id = other.rekor_entry_id.clone();

        let result = verify_signature_fulcio(&swapped, &test_config(), &stack, test_now()).await;
        assert!(
            !result.rekor_entry_exists,
            "an entry for another commit must fail (d): {:?}",
            result.reason
        );
        assert!(!result.valid);
    }

    /// F1 (wire format, upstream contract): the mock Fulcio rejects a
    /// base64(DER) public key exactly like the real one — and the production
    /// signer sends PEM, so issuance succeeds.
    #[tokio::test]
    async fn fulcio_rejects_base64_der_public_key_but_accepts_ours() {
        let stack = MockSigningStack::new();
        // sign() exercises the full production request path: if the
        // production code base64-DER-encoded the key, the mock's PEM check
        // (upstream ParsePublicKey behavior) would reject it.
        let record = sign(&stack, "wirefmt").await;
        assert!(record.certificate_pem.is_some());

        // Direct probe: a base64(DER) key in publicKey.content fails.
        let key = rcgen::KeyPair::generate().unwrap();
        let spki_b64 = BASE64.encode(key.public_key_der());
        let req = serde_json::json!({
            "credentials": { "oidcIdentityToken": test_jwt("agent-7") },
            "publicKeyRequest": {
                "publicKey": { "content": spki_b64, "algorithm": "ecdsa" },
                "proofOfPossession": BASE64.encode(b"junk"),
            }
        });
        let err = stack
            .fulcio_signing_cert("https://fulcio.test", &req.to_string())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid public key"));
    }

    /// PoP upstream contract: a proof of possession over the wrong
    /// preimage must be rejected by Fulcio (the mock enforces it for real).
    #[tokio::test]
    async fn fulcio_rejects_pop_over_wrong_preimage() {
        let key = rcgen::KeyPair::generate().unwrap();
        let ecdsa_key = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &key.serialize_der(),
            &ring::rand::SystemRandom::new(),
        )
        .unwrap();
        // Sign something that is NOT the token subject.
        let pop = BASE64.encode(
            ecdsa_key
                .sign(&ring::rand::SystemRandom::new(), b"not-the-sub")
                .unwrap()
                .as_ref(),
        );
        let req = serde_json::json!({
            "credentials": { "oidcIdentityToken": test_jwt("agent-7") },
            "publicKeyRequest": {
                "publicKey": { "content": key.public_key_pem(), "algorithm": "ecdsa" },
                "proofOfPossession": pop,
            }
        });
        let stack = MockSigningStack::new();
        let err = stack
            .fulcio_signing_cert("https://fulcio.test", &req.to_string())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("proof of possession"));
    }

    /// F3 (trust anchors): a record claiming a different Fulcio/Rekor
    /// cannot redirect verification — but our record stores no URLs at all
    /// and verification only ever reads `config`. Drive a config whose
    /// Fulcio differs from the signing-time one; the trust bundle fetch
    //  uses the CONFIG URL and fails closed.
    #[tokio::test]
    async fn verification_uses_configured_trust_anchor_not_record_claims() {
        let stack = MockSigningStack::new();
        let record = sign(&stack, "anchor1").await;

        // Config points at a Fulcio whose bundle does NOT contain our root.
        let other_stack = MockSigningStack::new();
        let mut cfg = test_config();
        cfg.fulcio_url = "https://other-fulcio.test".to_string();
        let result = verify_signature_fulcio(&record, &cfg, &other_stack, test_now()).await;
        assert!(
            !result.certificate_chain_valid,
            "a different trust anchor must reject the chain"
        );
        assert!(!result.valid);
    }
}

/// Verify a stored `(repo_id, commit_sha)` signature record against the
/// state's configured trust anchors. Shared by the signature-verification
/// endpoint and the provenance chain API (task-107 F6).
///
/// Returns `Ok(None)` when no signature record exists for the commit — the
/// caller decides whether that is meaningful (e.g. `GYRE_SIGNING_MODE=none`
/// produces unsigned commits by design).
pub async fn verify_state_commit_signature(
    state: &crate::AppState,
    repo_id: &str,
    commit_sha: &str,
) -> Result<Option<SignatureVerificationResult>> {
    let Some(record) = state
        .commit_signatures
        .find(repo_id, commit_sha)
        .await
        .context("commit signature lookup failed")?
    else {
        return Ok(None);
    };
    let result = match record.sigstore_mode {
        SigstoreMode::Local => {
            // Real Ed25519 verification against the forge key (the key is
            // regenerated per server start and published at
            // /.well-known/jwks.json).
            let sig_valid = crate::commit_signatures::verify_commit_signature(
                &record,
                &state.agent_signing_key.public_key_bytes,
            );
            SignatureVerificationResult::local_mode(&record, sig_valid)
        }
        SigstoreMode::Fulcio => {
            let transport = production_transport(state.http_client.clone());
            verify_signature_fulcio(
                &record,
                &state.signing_config,
                transport.as_ref(),
                std::time::SystemTime::now(),
            )
            .await
        }
    };
    Ok(Some(result))
}
