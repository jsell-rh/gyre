//! Upstream wire-contract conformance probe for task-107 keyless signing.
//!
//! The unit tests in `sigstore.rs` run against `MockSigningStack`, which
//! lives next to the production code and can encode the same wire-format
//! misunderstanding the production decoder has — every test then passes
//! while every REAL Fulcio instance rejects or confuses the client. That is
//! exactly what the task-107 review found: both production and mock treated
//! `signedCertificateEmbeddedSct.chain.certificates` and the v2 trust
//! bundle's `chains` as base64(DER), while upstream Fulcio's proto declares
//! the field `repeated string` and the server fills it with PEM text
//! (`fulcio.proto`: "The PEM-encoded certificate chain, ordered from leaf to
//! intermediate to root"; `pkg/server/grpc_server.go`:
//! `Certificates: append([]string{finalPEM}, finalChainPEM...)` with
//! CertPEM()/ChainPEM() PEM-marshaling; reference client sigstore-rs models
//! it as `Vec<Pem>`).
//!
//! This probe is deliberately INDEPENDENT of the in-module mock: it builds
//! its own Fulcio from the upstream proto documentation — response chain
//! elements and trust-bundle elements are PEM STRINGS — and drives the REAL
//! production `sign_commit_keyless` and `verify_signature_fulcio`. A
//! regression to base64(DER)-only decoding fails here even though the unit
//! suite (mock and code moving together) stays green.
//!
//! Live-instance verification (blocked in sandboxed CI by network policy,
//! run on a networked host): the public Fulcio trust bundle can be fetched
//! and inspected directly:
//!   curl -s https://fulcio.sigstore.dev/api/v2/trustBundle | jq '.chains[0][0]'
//! The element must be a PEM string beginning `-----BEGIN CERTIFICATE-----`,
//! not base64. Feeding that bundle JSON into a `FixedBundle` transport and
//! verifying any Fulcio-issued record exercises the same decode path.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_server::commit_signatures::{SigningAttribution, SigningConfig, SigningMode};
use gyre_server::sigstore::{
    sign_commit_keyless, verify_signature_fulcio, SigningHttpTransport,
};
use anyhow::{anyhow, Context, Result};

// ── A Fulcio built from the upstream proto documentation ─────────────────────

struct UpstreamFulcio {
    /// The CA that issues leaf certificates (plays the Fulcio CA).
    ca_key: rcgen::KeyPair,
    ca_cert: rcgen::Certificate,
    /// Entries recorded by the mock Rekor: uuid → hashedrekord JSON.
    rekor_entries: parking_lot::Mutex<std::collections::HashMap<String, serde_json::Value>>,
}

impl UpstreamFulcio {
    fn new() -> Self {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec![]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "upstream-probe-fulcio-ca");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::DigitalSignature,
            rcgen::KeyUsagePurpose::KeyCertSign,
        ];
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2050, 1, 1);
        let ca_cert = params.self_signed(&ca_key).unwrap();
        Self {
            ca_key,
            ca_cert,
            rekor_entries: Default::default(),
        }
    }

    /// Trust bundle EXACTLY as upstream serves it: `chains` is a list of
    /// chains, each chain a list of PEM STRINGS.
    fn trust_bundle_json(&self) -> String {
        serde_json::json!({
            "chains": [[ self.ca_cert.pem() ]]
        })
        .to_string()
    }
}

#[async_trait]
impl SigningHttpTransport for UpstreamFulcio {
    async fn fulcio_signing_cert(&self, _url: &str, body: &str) -> Result<String> {
        let req: serde_json::Value =
            serde_json::from_str(body).context("probe: request must be protojson")?;

        // Upstream ParsePublicKey contract: PEM text or raw DER.
        let pk_pem = req["publicKeyRequest"]["publicKey"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("probe: publicKey.content missing"))?;
        if !pk_pem.contains("-----BEGIN PUBLIC KEY-----") {
            return Err(anyhow!("probe: Fulcio rejects non-PEM publicKey.content"));
        }

        // Verify the proof of possession like the real challenges check.
        let jwt = req["credentials"]["oidcIdentityToken"]
            .as_str()
            .ok_or_else(|| anyhow!("probe: oidcIdentityToken missing"))?;
        let sub = extract_sub(jwt)?;
        let pop_b64 = req["publicKeyRequest"]["proofOfPossession"]
            .as_str()
            .ok_or_else(|| anyhow!("probe: proofOfPossession missing"))?;
        let pop = BASE64
            .decode(pop_b64)
            .context("probe: PoP must be base64")?;
        let spki = rcgen::SubjectPublicKeyInfo::from_pem(pk_pem)
            .context("probe: publicKey.content must parse as PEM SPKI")?;
        use rcgen::PublicKeyData as _;
        use ring::signature::UnparsedPublicKey;
        let vk = UnparsedPublicKey::new(
            &ring::signature::ECDSA_P256_SHA256_ASN1,
            spki.der_bytes(),
        );
        vk.verify(sub.as_bytes(), &pop)
            .map_err(|_| anyhow!("probe: proof of possession failed"))?;

        // Issue the leaf and build the response EXACTLY as upstream does:
        // `Certificates: append([]string{finalPEM}, finalChainPEM...)` —
        // every element is a PEM STRING.
        let mut params = rcgen::CertificateParams::new(vec![]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, sub.clone());
        params.subject_alt_names = vec![rcgen::SanType::URI(
            sub.clone().try_into().expect("SAN must be IA5"),
        )];
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        params.not_after = rcgen::date_time_ymd(2050, 1, 1);
        let spki = rcgen::SubjectPublicKeyInfo::from_pem(pk_pem).unwrap();
        let leaf = params
            .signed_by(&spki, &self.ca_cert, &self.ca_key)
            .unwrap();
        let certificates = vec![leaf.pem(), self.ca_cert.pem()];
        Ok(serde_json::json!({
            "signedCertificateEmbeddedSct": { "chain": { "certificates": certificates } }
        })
        .to_string())
    }

    async fn fulcio_trust_bundle(&self, _url: &str) -> Result<String> {
        Ok(self.trust_bundle_json())
    }

    async fn rekor_post_entry(&self, _url: &str, body: &str) -> Result<String> {
        let entry: serde_json::Value =
            serde_json::from_str(body).context("probe: hashedrekord JSON required")?;
        let uuid = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(body.as_bytes()));
        self.rekor_entries.lock().insert(uuid.clone(), entry);
        let body_b64 = BASE64.encode(body);
        Ok(serde_json::json!({ uuid.clone(): { "body": body_b64 } }).to_string())
    }

    async fn rekor_get_entry(&self, _url: &str, uuid: &str) -> Result<String> {
        let entries = self.rekor_entries.lock();
        let entry = entries
            .get(uuid)
            .ok_or_else(|| anyhow!("404: entry not found"))?;
        let body_b64 = BASE64.encode(serde_json::to_string(entry).unwrap());
        Ok(serde_json::json!({ uuid: { "body": body_b64 } }).to_string())
    }
}

fn extract_sub(jwt: &str) -> Result<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("JWT has no payload"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .context("payload not base64url")?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)?;
    v.get("sub")
        .and_then(|s| s.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("no sub claim"))
}

fn probe_jwt(sub: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA"}"#);
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({"sub": sub, "iss": "http://gyre"})).unwrap(),
    );
    format!("{header}.{payload}.sig")
}

fn probe_config() -> SigningConfig {
    SigningConfig {
        mode: SigningMode::Fulcio,
        fulcio_url: "https://fulcio-upstream-probe.test".to_string(),
        rekor_url: "https://rekor-upstream-probe.test".to_string(),
    }
}

fn probe_now() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_730_000_000)
}

/// The decisive probe: the REAL production signer must obtain a certificate
/// from a Fulcio whose response chain is PEM STRINGS (the upstream proto
/// contract). Before the fix, `sign_commit_keyless` failed here with
/// "fulcio chain element is not base64 DER: Invalid symbol 45, offset 0"
/// (45 = '-' of "-----BEGIN") and every real GYRE_SIGNING_MODE=fulcio
/// deployment silently fell back to local signing.
#[tokio::test]
async fn production_signs_against_upstream_pem_chain() {
    let fulcio = UpstreamFulcio::new();
    let signed = sign_commit_keyless(
        "repo-probe",
        "deadbeef",
        &SigningAttribution {
            agent_id: "agent-probe".to_string(),
            task_id: "task-probe".to_string(),
            spawned_by: "user-probe".to_string(),
        },
        &probe_jwt("agent-probe"),
        &probe_config(),
        &fulcio,
    )
    .await
    .expect(
        "production keyless signing must succeed against a Fulcio that serves \
         the upstream PEM-string chain contract",
    );
    assert_eq!(signed.record.sigstore_mode, gyre_ports::commit_signature_repo::SigstoreMode::Fulcio);
    assert!(signed.record.certificate_pem.is_some());
    assert!(signed.record.rekor_entry_id.is_some());
}

/// The full round trip against the upstream contract: sign AND verify, with
/// the trust bundle served as PEM strings too. Before the fix, phase (b)
/// failed against every real trust bundle for the same reason.
#[tokio::test]
async fn production_verifies_against_upstream_pem_bundle() {
    let fulcio = UpstreamFulcio::new();
    let signed = sign_commit_keyless(
        "repo-probe",
        "cafe1234",
        &SigningAttribution {
            agent_id: "agent-probe".to_string(),
            task_id: "task-probe".to_string(),
            spawned_by: "user-probe".to_string(),
        },
        &probe_jwt("agent-probe"),
        &probe_config(),
        &fulcio,
    )
    .await
    .expect("signing leg must succeed first");

    let result = verify_signature_fulcio(&signed.record, &probe_config(), &fulcio, probe_now()).await;
    assert!(result.valid, "verification against the upstream PEM bundle must pass all four phases: {:?}", result.reason);
    assert!(result.certificate_chain_valid);
    assert!(result.signature_valid);
    assert!(result.subject_matches);
    assert!(result.rekor_entry_exists);
}

/// Belt-and-braces for the raw-JSON contract: the trust bundle the probe
/// serves must actually contain PEM strings (guards the guard — if the
/// probe's own JSON drifted back to base64, the two tests above would
/// quietly lose their meaning).
#[test]
fn probe_bundle_actually_encodes_pem_strings() {
    let fulcio = UpstreamFulcio::new();
    let bundle: serde_json::Value = serde_json::from_str(&fulcio.trust_bundle_json()).unwrap();
    let first = bundle["chains"][0][0]
        .as_str()
        .expect("chain element must be a JSON string");
    assert!(
        first.starts_with("-----BEGIN CERTIFICATE-----"),
        "probe bundle must encode the upstream PEM-string contract, got: {first}"
    );
}
