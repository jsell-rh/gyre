//! The Gyre trust-domain CA: issues SPIFFE-conformant X.509-SVIDs for agent
//! workloads when no external SPIRE server / Workload API is configured.
//!
//! Conformance with the SPIFFE X.509-SVID specification:
//! - Leaf and CA keys are ECDSA P-256 (the spec permits ECDSA or RSA).
//! - The leaf carries exactly one SAN: a URI SAN holding the SPIFFE ID.
//! - The leaf is not a CA; its key usage is digitalSignature (SVIDs
//!   authenticate workloads, they do not sign certificates).
//! - The CA certificate is self-signed with `keyCertSign`/`cRLSign` and no
//!   EKUs (the spec forbids EKUs on signing CAs).
//! - SVIDs are short-lived (default 1 hour) — see BASE_SVID_TTL_SECS.

use crate::id::SpiffeId;
use base64::Engine as _;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use time::OffsetDateTime;

/// Default SVID lifetime in seconds (SPIFFE X.509-SVID specification:
/// workloads should expect short-lived SVIDs; the reference default is
/// one hour).
pub const BASE_SVID_TTL_SECS: u64 = 3600;

/// A Gyre-operated trust-domain CA.
///
/// Holds the signing key in memory; the caller persists
/// [`TrustDomainCa::serialize`] output (PKCS#8 key + CA certificate PEM)
/// in durable storage so the CA — and with it every issued SVID and the
/// published trust bundle — survives restarts.
pub struct TrustDomainCa {
    key: KeyPair,
    /// Signing template (reconstructed from the persisted certificate so
    /// leaves issued after a restart chain to the same CA subject).
    cert: Certificate,
    /// CA certificate DER (the trust-bundle / verification anchor).
    cert_der: Vec<u8>,
}

/// Serialized CA material for persistence: PKCS#8 DER private key (base64)
/// and the CA certificate PEM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerializedCa {
    /// Base64-encoded PKCS#8 DER private key.
    pub key_pkcs8_b64: String,
    /// PEM-encoded self-signed CA certificate.
    pub cert_pem: String,
}

/// An issued X.509-SVID.
#[derive(Debug, Clone)]
pub struct IssuedSvid {
    /// The SPIFFE ID carried as the leaf's URI SAN.
    pub spiffe_id: SpiffeId,
    /// Leaf + CA chain, PEM, leaf first (the presentation format).
    pub chain_pem: String,
    /// Leaf certificate PEM.
    pub leaf_pem: String,
    /// The SVID private key, PEM (PKCS#8). Handed to the workload; the
    /// server does not retain it.
    pub private_key_pem: String,
    /// Leaf serial number (hex).
    pub serial: String,
    /// Validity window (Unix epoch seconds).
    pub not_before: i64,
    pub not_after: i64,
}

impl TrustDomainCa {
    /// Generate a fresh trust-domain CA.
    pub fn generate(trust_domain: &str) -> anyhow::Result<Self> {
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        // The SPIFFE X.509-SVID specification forbids EKUs on signing CAs.
        params.extended_key_usages = vec![];
        let mut dn = DistinguishedName::new();
        dn.push(
            DnType::CommonName,
            format!("Gyre trust-domain CA ({trust_domain})"),
        );
        params.distinguished_name = dn;
        let not_before = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        let not_after = OffsetDateTime::now_utc() + time::Duration::days(3650);
        params.not_before = not_before;
        params.not_after = not_after;
        let cert = params.self_signed(&key)?;
        let cert_der = cert.der().to_vec();
        Ok(Self {
            key,
            cert,
            cert_der,
        })
    }

    /// Reconstruct the CA from persisted material.
    ///
    /// The signing template is re-self-signed from the persisted
    /// certificate's parsed parameters with the persisted key (rcgen's
    /// documented import path); the PERSISTED certificate DER remains the
    /// trust anchor, and the key/certificate pair is checked to match so a
    /// mismatched store cannot silently re-anchor the bundle.
    pub fn from_serialized(serialized: &SerializedCa) -> anyhow::Result<Self> {
        let key_der = base64::engine::general_purpose::STANDARD
            .decode(serialized.key_pkcs8_b64.trim())
            .map_err(|e| anyhow::anyhow!("CA key is not valid base64: {e}"))?;
        let key = KeyPair::from_pkcs8_der_and_sign_algo(
            &rustls_pki_types::PrivatePkcs8KeyDer::from(key_der.as_slice()),
            &rcgen::PKCS_ECDSA_P256_SHA256,
        )
        .map_err(|e| anyhow::anyhow!("CA key is not valid ECDSA P-256 PKCS#8: {e}"))?;

        let params = CertificateParams::from_ca_cert_pem(serialized.cert_pem.trim())
            .map_err(|e| anyhow::anyhow!("persisted CA certificate is unreadable: {e}"))?;
        // from_ca_cert_pem keeps the parsed validity window and DN; the
        // re-sign below only rebuilds the issuer template.
        let template = params
            .self_signed(&key)
            .map_err(|e| anyhow::anyhow!("CA template re-signing failed: {e}"))?;

        let der = pem::parse(serialized.cert_pem.trim())
            .map_err(|e| anyhow::anyhow!("CA certificate PEM invalid: {e}"))?
            .contents()
            .to_vec();
        // Key/certificate consistency: the reconstructed template's public
        // key must equal the persisted certificate's SubjectPublicKeyInfo.
        let (_, parsed) = x509_parser::parse_x509_certificate(&der)
            .map_err(|e| anyhow::anyhow!("CA certificate DER invalid: {e}"))?;
        if parsed.subject_pki.raw != key.public_key_der() {
            return Err(anyhow::anyhow!(
                "persisted CA key does not match the persisted CA certificate"
            ));
        }
        Ok(Self {
            key,
            cert: template,
            cert_der: der,
        })
    }

    /// Serialize for persistence.
    pub fn serialize(&self) -> SerializedCa {
        SerializedCa {
            key_pkcs8_b64: base64::engine::general_purpose::STANDARD
                .encode(self.key.serialize_der()),
            cert_pem: self.cert.pem(),
        }
    }

    /// The CA certificate in PEM form (trust bundle element).
    pub fn cert_pem(&self) -> String {
        pem::encode(&pem::Pem::new("CERTIFICATE", self.cert_der.clone()))
    }

    /// The CA certificate DER (verification anchor element).
    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }

    /// Issue an X.509-SVID for `spiffe_id`, valid for `ttl_secs` from now.
    pub fn issue_svid(&self, spiffe_id: &SpiffeId, ttl_secs: u64) -> anyhow::Result<IssuedSvid> {
        let not_before = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        let not_after = OffsetDateTime::now_utc() + time::Duration::seconds(ttl_secs.max(1) as i64);
        self.issue_svid_with_validity(spiffe_id, not_before, not_after)
    }

    /// Issue an X.509-SVID with an explicit validity window (used by the
    /// issuance path and by tests exercising expiry enforcement).
    pub fn issue_svid_with_validity(
        &self,
        spiffe_id: &SpiffeId,
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
    ) -> anyhow::Result<IssuedSvid> {
        let serial_bytes = random_serial();
        self.issue_svid_with_serial(spiffe_id, not_before, not_after, serial_bytes)
    }

    /// Issue an X.509-SVID with an explicit serial (the random-serial core
    /// shared by [`TrustDomainCa::issue_svid_with_validity`]; tests force
    /// specific serial forms to pin the issuance/verification format
    /// contract).
    fn issue_svid_with_serial(
        &self,
        spiffe_id: &SpiffeId,
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
        serial_bytes: [u8; 16],
    ) -> anyhow::Result<IssuedSvid> {
        let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        let mut params = CertificateParams::default();
        // SPIFFE X.509-SVID: exactly one SAN — the URI SAN holding the
        // SPIFFE ID. No DNS/IP/email SANs, no other name forms.
        params.subject_alt_names =
            vec![SanType::URI(spiffe_id.to_string().try_into().map_err(
                |e| anyhow::anyhow!("SPIFFE ID is not a valid IA5String: {e:?}"),
            )?)];
        // Leaf is NOT a CA.
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        // ClientAuth EKU: the SVID authenticates the workload to servers
        // (the SPIFFE spec's mTLS profile).
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, spiffe_id.to_string());
        params.distinguished_name = dn;
        params.not_before = not_before;
        params.not_after = not_after;
        params.serial_number = Some(rcgen::SerialNumber::from_slice(&serial_bytes));

        let leaf = params.signed_by(&leaf_key, &self.cert, &self.key)?;
        let leaf_pem = leaf.pem();
        let ca_pem = self.cert_pem();
        let chain_pem = format!("{leaf_pem}\n{ca_pem}");
        Ok(IssuedSvid {
            spiffe_id: spiffe_id.clone(),
            chain_pem,
            leaf_pem,
            private_key_pem: leaf_key.serialize_pem(),
            // Canonical (big-integer) hex — the SAME form verification
            // extracts from the parsed leaf, so a genuine SVID can never
            // be rejected for a serial-encoding mismatch.
            serial: crate::canonical_serial_hex(&serial_bytes),
            not_before: not_before.unix_timestamp(),
            not_after: not_after.unix_timestamp(),
        })
    }
}

fn random_serial() -> [u8; 16] {
    // Random 128-bit serial (positive and top-byte-nonzero: leading byte
    // forced into 1..=127 so the DER INTEGER has no sign/leading-zero
    // complications and the serial's hex form is stable).
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = [0u8; 16];
    if SystemRandom::new().fill(&mut buf).is_err() {
        // Fallback: time + pid mixing (serial uniqueness is the
        // requirement, not secrecy).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf[..8].copy_from_slice(&now.to_le_bytes()[..8]);
        buf[8..].copy_from_slice(&(std::process::id() as u64).to_le_bytes());
    }
    buf[0] = (buf[0] % 127) + 1;
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{verify_svid, ExpectedIdentity};

    fn expected_for(id: &SpiffeId) -> ExpectedIdentity {
        let p = id.agent_path().unwrap();
        ExpectedIdentity {
            trust_domain: id.trust_domain.clone(),
            role: p.role,
            session_id: p.session_id,
        }
    }

    #[test]
    fn issues_svid_that_verifies_against_its_bundle() {
        let ca = TrustDomainCa::generate("gyre.example.com").unwrap();
        let id = SpiffeId::agent("gyre.example.com", "worker", "sess-1").unwrap();
        let svid = ca.issue_svid(&id, BASE_SVID_TTL_SECS).unwrap();

        // Chain PEM: leaf first, CA second.
        assert!(svid.chain_pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert_eq!(svid.chain_pem.matches("BEGIN CERTIFICATE").count(), 2);

        let anchors = vec![ca.cert_der().to_vec()];
        let verified = verify_svid(
            &svid.chain_pem,
            &anchors,
            &expected_for(&id),
            std::time::SystemTime::now(),
        )
        .unwrap();
        assert_eq!(verified.spiffe_id, id);
        assert_eq!(verified.serial, svid.serial);
        assert!(verified.not_after > verified.not_before);
    }

    /// Regression (task-261 review): issuance formatted the serial as
    /// fixed-width `hex::encode(serial_bytes)` while verification
    /// formatted it as a big-integer `{:x}` — a serial whose top byte is
    /// `0x01..=0x0F` carried a leading zero nibble on one side only, so
    /// ~12% of genuinely issued SVIDs were rejected as serial mismatches
    /// (agent heartbeats permanently failing). Both sides now format
    /// through `canonical_serial_hex`; this test forces the worst case
    /// deterministically instead of relying on the random ~12%.
    #[test]
    fn issued_and_verified_serials_agree_on_leading_zero_nibbles() {
        let ca = TrustDomainCa::generate("gyre.example.com").unwrap();
        let id = SpiffeId::agent("gyre.example.com", "worker", "sess-lz").unwrap();
        // Top byte 0x01: fixed-width hex would be "01..." — one leading
        // zero nibble the big-integer form strips.
        let mut serial = [0u8; 16];
        serial[0] = 0x01;
        for (i, b) in serial.iter_mut().enumerate().skip(1) {
            *b = (i as u8).wrapping_mul(37).wrapping_add(5);
        }
        let not_before = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        let not_after = OffsetDateTime::now_utc() + time::Duration::hours(1);
        let svid = ca
            .issue_svid_with_serial(&id, not_before, not_after, serial)
            .unwrap();
        assert_eq!(
            svid.serial.len(),
            31,
            "issued serial must be the stripped form (31 nibbles for a top byte of 0x01), got {}",
            svid.serial
        );
        assert!(!svid.serial.starts_with('0'));

        let anchors = vec![ca.cert_der().to_vec()];
        let verified = verify_svid(
            &svid.chain_pem,
            &anchors,
            &expected_for(&id),
            std::time::SystemTime::now(),
        )
        .unwrap();
        assert_eq!(
            verified.serial, svid.serial,
            "issuance and verification must agree on the serial format"
        );
    }

    #[test]
    fn ca_survives_serialization_roundtrip() {
        let ca = TrustDomainCa::generate("gyre.example.com").unwrap();
        let id = SpiffeId::agent("gyre.example.com", "security", "sess-2").unwrap();
        let svid1 = ca.issue_svid(&id, BASE_SVID_TTL_SECS).unwrap();

        let serialized = ca.serialize();
        let restored = TrustDomainCa::from_serialized(&serialized).unwrap();
        // The anchor DER must be preserved across restart.
        assert_eq!(restored.cert_der(), ca.cert_der());
        // A post-restart SVID must verify against the SAME bundle anchor.
        let svid2 = restored.issue_svid(&id, BASE_SVID_TTL_SECS).unwrap();
        let anchors = vec![restored.cert_der().to_vec()];
        for svid in [&svid1, &svid2] {
            assert!(verify_svid(
                &svid.chain_pem,
                &anchors,
                &expected_for(&id),
                std::time::SystemTime::now()
            )
            .is_ok());
        }
    }

    #[test]
    fn from_serialized_rejects_mismatched_key_pair() {
        let ca = TrustDomainCa::generate("gyre.example.com").unwrap();
        let other = TrustDomainCa::generate("gyre.example.com").unwrap();
        let mut serialized = ca.serialize();
        serialized.key_pkcs8_b64 = other.serialize().key_pkcs8_b64;
        assert!(TrustDomainCa::from_serialized(&serialized).is_err());
    }
}
