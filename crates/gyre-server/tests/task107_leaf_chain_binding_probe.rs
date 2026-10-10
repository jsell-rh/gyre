//! TEMPORARY review probe (deleted after run): does verify_signature_fulcio
//! bind the leaf certificate used for signature/subject/Rekor verification to
//! the chain it validates in phase (b)?
//!
//! Attack simulated (requires only record tampering — e.g. DB write):
//! - keep the ORIGINAL legit Fulcio chain in certificate_chain_pem (phase b passes)
//! - swap certificate_pem to a ROGUE self-signed cert with the same SAN
//! - swap signature to one made with the rogue key over the same commit_sha
//! - POST a matching rogue entry to Rekor and point rekor_entry_id at it
//!
//! If all four phases pass, the verification never proves the SIGNING
//! certificate was issued by the trusted Fulcio — only that SOME stored chain
//! was. Task plan item 3b is then not enforced.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_server::commit_signatures::{SigningAttribution, SigningConfig, SigningMode};
use gyre_server::sigstore::{sign_commit_keyless, verify_signature_fulcio, SigningHttpTransport};
use anyhow::{anyhow, Context, Result};

// ── Upstream-contract Fulcio (same as the committed conformance probe) ───────

struct UpstreamFulcio {
    ca_key: rcgen::KeyPair,
    ca_cert: rcgen::Certificate,
    rekor_entries: parking_lot::Mutex<std::collections::HashMap<String, serde_json::Value>>,
}

impl UpstreamFulcio {
    fn new() -> Self {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec![]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "probe-fulcio-ca");
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

    fn trust_bundle_json(&self) -> String {
        serde_json::json!({ "chains": [[ self.ca_cert.pem() ]] }).to_string()
    }
}

#[async_trait]
impl SigningHttpTransport for UpstreamFulcio {
    async fn fulcio_signing_cert(&self, _url: &str, body: &str) -> Result<String> {
        let req: serde_json::Value =
            serde_json::from_str(body).context("request must be protojson")?;
        let pk_pem = req["publicKeyRequest"]["publicKey"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("publicKey.content missing"))?;
        if !pk_pem.contains("-----BEGIN PUBLIC KEY-----") {
            return Err(anyhow!("reject non-PEM publicKey.content"));
        }
        let jwt = req["credentials"]["oidcIdentityToken"]
            .as_str()
            .ok_or_else(|| anyhow!("oidcIdentityToken missing"))?;
        let sub = extract_sub(jwt)?;
        let pop_b64 = req["publicKeyRequest"]["proofOfPossession"]
            .as_str()
            .ok_or_else(|| anyhow!("proofOfPossession missing"))?;
        let pop = BASE64.decode(pop_b64).context("PoP must be base64")?;
        let spki = rcgen::SubjectPublicKeyInfo::from_pem(pk_pem)?;
        use rcgen::PublicKeyData as _;
        use ring::signature::UnparsedPublicKey;
        let vk = UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, spki.der_bytes());
        vk.verify(sub.as_bytes(), &pop).map_err(|_| anyhow!("PoP failed"))?;

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
        let leaf = params.signed_by(&spki, &self.ca_cert, &self.ca_key).unwrap();
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
        let entry: serde_json::Value = serde_json::from_str(body)?;
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
    let payload = jwt.split('.').nth(1).ok_or_else(|| anyhow!("no payload"))?;
    let bytes = URL_SAFE_NO_PAD.decode(payload)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)?;
    v.get("sub").and_then(|s| s.as_str()).map(String::from).ok_or_else(|| anyhow!("no sub"))
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
        fulcio_url: "https://fulcio-probe.test".to_string(),
        rekor_url: "https://rekor-probe.test".to_string(),
    }
}

fn probe_now() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_730_000_000)
}

fn commit_digest(sha: &str) -> String {
    hex::encode(<sha2::Sha256 as sha2::Digest>::digest(sha.as_bytes()))
}

#[tokio::test]
async fn leaf_chain_decoupling_allows_forged_verification() {
    let fulcio = UpstreamFulcio::new();

    // 1. Real, honest signing through the production path.
    let signed = sign_commit_keyless(
        "repo-probe",
        "forgeme1",
        &SigningAttribution {
            agent_id: "agent-real".to_string(),
            task_id: "task-real".to_string(),
            spawned_by: "user-real".to_string(),
        },
        &probe_jwt("agent-real"),
        &probe_config(),
        &fulcio,
    )
    .await
    .expect("honest signing must succeed");

    // Sanity: honest record verifies.
    let honest = verify_signature_fulcio(&signed.record, &probe_config(), &fulcio, probe_now()).await;
    assert!(honest.valid, "honest record must verify: {:?}", honest.reason);

    // 2. Build the ROGUE leaf: self-signed, SAME SAN, brand-new key.
    let sub = signed.record.oidc_subject.clone();
    let rogue_key = rcgen::KeyPair::generate().unwrap();
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
    let rogue_cert = params.self_signed(&rogue_key).unwrap();
    let rogue_pem = rogue_cert.pem();

    // 3. Sign the SAME commit sha with the rogue key (DER ECDSA, like production).
    let ecdsa = ring::signature::EcdsaKeyPair::from_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        &rogue_key.serialize_der(),
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    let commit_sha = signed.record.commit_sha.clone();
    let rogue_sig = BASE64.encode(
        ecdsa
            .sign(&ring::rand::SystemRandom::new(), commit_sha.as_bytes())
            .unwrap()
            .as_ref(),
    );

    // 4. POST a rogue Rekor entry for (digest, rogue sig, rogue cert) and keep its UUID.
    let digest = commit_digest(&commit_sha);
    let rogue_entry = serde_json::json!({
        "kind": "hashedrekord",
        "apiVersion": "0.0.1",
        "spec": {
            "data": { "hash": { "algorithm": "sha256", "value": digest } },
            "signature": {
                "content": rogue_sig,
                "publicKey": { "content": BASE64.encode(rogue_pem.as_bytes()) }
            }
        }
    });
    let post_resp = fulcio
        .rekor_post_entry("https://rekor-probe.test", &rogue_entry.to_string())
        .await
        .unwrap();
    let uuid_map: serde_json::Value = serde_json::from_str(&post_resp).unwrap();
    let rogue_uuid = uuid_map.as_object().unwrap().keys().next().unwrap().clone();

    // 5. Forge the record: rogue leaf + rogue signature + rogue Rekor entry,
    //    but the ORIGINAL legit Fulcio chain (phase b input) untouched.
    let mut forged = signed.record.clone();
    forged.certificate_pem = Some(rogue_pem);
    forged.signature = rogue_sig;
    forged.rekor_entry_id = Some(rogue_uuid);
    // certificate_chain_pem, oidc_subject, signer_id, task_id, spawned_by: unchanged.

    // 6. The decisive question: does verification accept the forgery?
    let result = verify_signature_fulcio(&forged, &probe_config(), &fulcio, probe_now()).await;
    println!(
        "FORGED RECORD VERIFICATION: valid={} sig={} chain={} subject={} rekor={} reason={:?}",
        result.valid,
        result.signature_valid,
        result.certificate_chain_valid,
        result.subject_matches,
        result.rekor_entry_exists,
        result.reason
    );

    // If the verification is sound, the forged record MUST be rejected:
    // the leaf verifying the signature was never issued by the trusted Fulcio.
    // We print the outcome; the assert documents the EXPECTED (secure) behavior.
    // Review probe: a panic here means the forgery is REJECTED (good);
    // a pass means the forgery is ACCEPTED (defect).
    let accepted = result.valid;
    if accepted {
        panic!(
            "DEFECT: forged record (rogue self-signed leaf, rogue signature, rogue Rekor entry, \
             original legit chain) passes ALL FOUR verification phases — the signing certificate \
             is never bound to the validated chain"
        );
    }
}
