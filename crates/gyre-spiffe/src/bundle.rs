//! SPIFFE trust bundle serialization (SPIFFE Trust Domain Bundle format).
//!
//! A SPIFFE bundle for X.509-SVIDs is a JWK Set (RFC 7517) whose keys carry
//! `use: "svid"`. Each key is the public key of a CA that signs the trust
//! domain's X.509-SVIDs; the `x5c` member conveys the CA certificate chain
//! (base64 DER, leaf/root first) so verifiers can validate both the key and
//! the certificate binding. The server publishes this document at its
//! well-known endpoint; it is the sole verification anchor.

use serde_json::{json, Value};

/// PEM label for certificate blocks in SVID chains.
pub const CERT_PEM_LABEL: &str = "CERTIFICATE";

/// Build the SPIFFE bundle (JWK Set) for a set of CA certificates.
///
/// `ca_certs_pem` — PEM-encoded CA certificates (the anchors).
pub fn trust_bundle_jwks(ca_certs_pem: &[String]) -> Value {
    let mut keys = Vec::new();
    for pem_str in ca_certs_pem {
        if let Some(jwk) = ca_pem_to_jwk(pem_str) {
            keys.push(jwk);
        }
    }
    json!({ "keys": keys })
}

/// Parse a SPIFFE bundle (JWK Set) back into DER-encoded CA certificates.
///
/// Fails closed: every key must carry `use: "svid"` and an `x5c` chain whose
/// first element is the CA certificate; keys without `x5c` are skipped
/// (they may be JWT-SVID signing keys, which cannot anchor X.509-SVIDs).
pub fn parse_trust_bundle(bundle: &Value) -> Result<Vec<Vec<u8>>, String> {
    use base64::Engine as _;
    let keys = bundle
        .get("keys")
        .and_then(|k| k.as_array())
        .ok_or_else(|| "bundle is not a JWK set (missing keys array)".to_string())?;
    let mut certs = Vec::new();
    for key in keys {
        let use_ = key.get("use").and_then(|u| u.as_str()).unwrap_or("");
        if use_ != "svid" {
            return Err(format!("bundle key has use={use_:?}, expected \"svid\""));
        }
        let x5c = key
            .get("x5c")
            .and_then(|x| x.as_array())
            .ok_or_else(|| "bundle key has no x5c chain".to_string())?;
        let first = x5c
            .first()
            .and_then(|c| c.as_str())
            .ok_or_else(|| "bundle key x5c chain is empty".to_string())?;
        let der = base64::engine::general_purpose::STANDARD
            .decode(first)
            .map_err(|e| format!("bundle x5c element is not base64 DER: {e}"))?;
        certs.push(der);
    }
    if certs.is_empty() {
        return Err("bundle contains no X.509 anchors".to_string());
    }
    Ok(certs)
}

/// One CA certificate (PEM) → JWK with `use: "svid"`, EC/RSA public key
/// members, `x5c` chain, and a `kid` derived from the SPKI (stable across
/// re-serialization of the same CA).
fn ca_pem_to_jwk(pem_str: &str) -> Option<Value> {
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use base64::Engine as _;
    let der = pem::parse(pem_str).ok()?.contents().to_vec();
    let (_, cert) = x509_parser::parse_x509_certificate(&der).ok()?;
    let spki = cert.subject_pki.raw;

    // kid: SHA-256 of the SubjectPublicKeyInfo DER (hex, first 16 bytes).
    use sha2::Digest;
    let kid = {
        let d = sha2::Sha256::digest(spki);
        // split_at (not a fixed byte-index slice): the digest is 32 bytes.
        let (prefix, _) = d.split_at(8);
        hex::encode(prefix)
    };

    // Public key members by algorithm OID.
    let algorithm = cert.subject_pki.algorithm.algorithm.to_id_string();
    let (kty, extra) = match algorithm.as_str() {
        // ecPublicKey with prime256v1
        "1.2.840.10045.2.1" => {
            // Uncompressed EC point: 0x04 || X (32) || Y (32) for P-256.
            let point = cert.subject_pki.subject_public_key.data.as_ref();
            if point.len() != 65 || point[0] != 0x04 {
                return None;
            }
            // split_at (not fixed byte-index slices): the uncompressed
            // P-256 point is 0x04 || X (32) || Y (32).
            let (_, coords) = point.split_at(1);
            let (x, y) = coords.split_at(32);
            (
                "EC",
                json!({
                    "crv": "P-256",
                    "x": URL_SAFE_NO_PAD.encode(x),
                    "y": URL_SAFE_NO_PAD.encode(y),
                }),
            )
        }
        // rsaEncryption
        "1.2.840.113549.1.1.1" => {
            // RSA public key is a DER RSAPublicKey inside the SPKI BIT
            // STRING; the JWK `n`/`e` members need INTEGER parsing, which
            // x509-parser exposes via rsa::RsaPublicKey in its `verify`
            // feature only for signature checks. Gyre's CA is ECDSA-only;
            // for RSA anchors emit `x5c`-only members (the `n`/`e` JWK
            // members are optional when `x5c` is present).
            ("RSA", json!({}))
        }
        _ => return None,
    };

    let mut jwk = json!({
        "kty": kty,
        "use": "svid",
        "kid": kid,
        "x5c": [STANDARD.encode(&der)],
    });
    if let Value::Object(map) = &mut jwk {
        if let Value::Object(extra_map) = extra {
            for (k, v) in extra_map {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    Some(jwk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_roundtrips_a_generated_ca() {
        let ca = crate::ca::TrustDomainCa::generate("gyre.example.com").unwrap();
        let bundle = trust_bundle_jwks(&[ca.cert_pem()]);
        let keys = bundle["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["use"], "svid");
        assert_eq!(keys[0]["kty"], "EC");
        assert_eq!(keys[0]["crv"], "P-256");

        let anchors = parse_trust_bundle(&bundle).unwrap();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0], ca.cert_der());
    }

    #[test]
    fn parse_rejects_non_svid_use() {
        let bundle = json!({"keys": [{"kty": "EC", "use": "sig"}]});
        assert!(parse_trust_bundle(&bundle).is_err());
    }

    #[test]
    fn parse_rejects_keyless_bundle() {
        assert!(parse_trust_bundle(&json!({"keys": []})).is_err());
        assert!(parse_trust_bundle(&json!({})).is_err());
    }
}
