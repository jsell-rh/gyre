//! Cryptographic verification of a presented X.509-SVID against a trust
//! bundle — the Layer-1 "this workload is real" proof.
//!
//! Every check fails closed: a presented SVID is accepted only if
//! 1. it parses (PEM chain, leaf first),
//! 2. its leaf is cryptographically signed by one of the trust bundle's
//!    CA certificates (`verify_signature`, real ECDSA verification — an
//!    SVID signed by any key not in the bundle is rejected),
//! 3. the leaf and the signing CA are within their validity windows at
//!    verification time (not-yet-valid and expired SVIDs are rejected),
//! 4. the leaf carries exactly one URI SAN holding a well-formed SPIFFE ID
//!    (no other SAN name forms),
//! 5. the SPIFFE ID's trust domain and `/agent/{role}/session/{id}` path
//!    bind it to the expected agent session (foreign trust domains and
//!    other sessions are rejected).
//!
//! No self-reported metadata participates in the decision: the trust
//! bundle is the sole verification anchor.

use crate::id::{AgentPath, SpiffeId};
use std::time::SystemTime;
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::{GeneralName, ParsedExtension};

/// PEM label expected for certificate blocks in a presented SVID chain.
pub const CHAIN_PEM_LABEL: &str = "CERTIFICATE";

/// The identity a verification call expects the presented SVID to prove.
#[derive(Debug, Clone)]
pub struct ExpectedIdentity {
    /// The trust domain this server operates (`GYRE_SPIFFE_TRUST_DOMAIN`).
    pub trust_domain: String,
    /// The role segment the session was spawned under.
    pub role: String,
    /// The agent session id the SVID must name.
    pub session_id: String,
}

/// A successfully verified SVID.
#[derive(Debug, Clone)]
pub struct VerifiedSvid {
    pub spiffe_id: SpiffeId,
    pub agent_path: AgentPath,
    /// Leaf serial (hex).
    pub serial: String,
    pub not_before: i64,
    pub not_after: i64,
}

/// Why a presented SVID was rejected. Every variant is a fail-closed
/// rejection; `Display` names the check that failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerificationError {
    #[error("no SVID presented")]
    NoSvid,
    #[error("presented SVID is not a parseable PEM certificate chain")]
    MalformedPem,
    #[error("SVID chain is empty")]
    EmptyChain,
    #[error("SVID leaf is not signed by any key in the trust bundle")]
    UntrustedSigner,
    #[error("SVID is not yet valid (not_before={0})")]
    NotYetValid(i64),
    #[error("SVID has expired (not_after={0})")]
    Expired(i64),
    #[error("signing CA is not within its validity window")]
    CaValidity,
    #[error("SVID leaf has no URI SAN")]
    NoUriSan,
    #[error("SVID leaf must carry exactly one URI SAN, found {0}")]
    MultipleUriSans(usize),
    #[error("SVID leaf carries non-URI SAN name forms (forbidden by the SPIFFE X.509-SVID spec)")]
    ForbiddenSanForm,
    #[error("SPIFFE ID is malformed: {0}")]
    MalformedSpiffeId(String),
    #[error("SPIFFE ID names foreign trust domain {found:?}, expected {expected:?}")]
    ForeignTrustDomain { found: String, expected: String },
    #[error("SPIFFE ID path is not the agent-workload form /agent/{{role}}/session/{{id}}")]
    NotAgentPath,
    #[error("SPIFFE ID names session {found:?}, expected {expected:?}")]
    WrongSession { found: String, expected: String },
    #[error("SPIFFE ID names role {found:?}, expected {expected:?}")]
    WrongRole { found: String, expected: String },
}

/// Verify a presented X.509-SVID (PEM chain, leaf first) against the trust
/// bundle's CA certificates (DER) for the expected agent session at time
/// `now`.
pub fn verify_svid(
    presented_pem: &str,
    trust_bundle_ca_der: &[Vec<u8>],
    expected: &ExpectedIdentity,
    now: SystemTime,
) -> Result<VerifiedSvid, VerificationError> {
    let chain = verify_chain_and_extract_id(presented_pem, trust_bundle_ca_der, now)?;
    bind_to_expected(chain, expected)
}

/// Verify a presented X.509-SVID against the trust bundle, binding it to an
/// exact expected SPIFFE ID (used when the SVID was obtained from an
/// external SPIRE Workload API, where the operator — not Gyre — chose the
/// SPIFFE ID shape; the session binding is the issuance-time record).
pub fn verify_svid_for_record_id(
    presented_pem: &str,
    trust_bundle_ca_der: &[Vec<u8>],
    expected_id: &SpiffeId,
    now: SystemTime,
) -> Result<VerifiedSvid, VerificationError> {
    let chain = verify_chain_and_extract_id(presented_pem, trust_bundle_ca_der, now)?;
    if chain.spiffe_id != *expected_id {
        return Err(VerificationError::WrongSession {
            found: chain.spiffe_id.to_string(),
            expected: expected_id.to_string(),
        });
    }
    let spiffe_id = chain.spiffe_id;
    let agent_path = spiffe_id
        .agent_path()
        .ok_or(VerificationError::NotAgentPath)?;
    Ok(VerifiedSvid {
        spiffe_id,
        agent_path,
        serial: chain.serial,
        not_before: chain.not_before,
        not_after: chain.not_after,
    })
}

/// Extract the (cryptographically UNVERIFIED) SPIFFE ID from a presented
/// SVID chain's leaf URI SAN.
///
/// SPIFFE federation use only: the trust domain of the extracted ID
/// selects which exchanged trust bundle the full verification runs
/// against (the SPIFFE federation model — bundle endpoints keyed by
/// trust domain). The result is never trusted on its own: it is a claim
/// from the presented certificate, and only [`verify_svid`] against the
/// selected bundle's anchors turns it into an attested identity.
pub fn extract_spiffe_id(presented_pem: &str) -> Result<SpiffeId, VerificationError> {
    if presented_pem.trim().is_empty() {
        return Err(VerificationError::NoSvid);
    }
    let chain = parse_pem_chain(presented_pem).ok_or(VerificationError::MalformedPem)?;
    let leaf_der = chain.first().ok_or(VerificationError::EmptyChain)?;
    let (_, leaf) = x509_parser::parse_x509_certificate(leaf_der)
        .map_err(|_| VerificationError::MalformedPem)?;
    extract_id_from_leaf(&leaf)
}

/// Checks (1)-(4): parse, chain signature against the bundle anchors,
/// validity windows, single-URI-SAN extraction. Returns the parsed leaf and
/// its SPIFFE ID.
fn verify_chain_and_extract_id(
    presented_pem: &str,
    trust_bundle_ca_der: &[Vec<u8>],
    now: SystemTime,
) -> Result<ChainVerified, VerificationError> {
    if presented_pem.trim().is_empty() {
        return Err(VerificationError::NoSvid);
    }
    if trust_bundle_ca_der.is_empty() {
        // Fail closed: no anchor means nothing can be verified.
        return Err(VerificationError::UntrustedSigner);
    }

    let chain = parse_pem_chain(presented_pem).ok_or(VerificationError::MalformedPem)?;
    let leaf_der = chain.first().ok_or(VerificationError::EmptyChain)?;
    let (_, leaf) = x509_parser::parse_x509_certificate(leaf_der)
        .map_err(|_| VerificationError::MalformedPem)?;

    // Parse the trust bundle anchors.
    let mut anchors = Vec::with_capacity(trust_bundle_ca_der.len());
    for der in trust_bundle_ca_der {
        if let Ok((_, cert)) = x509_parser::parse_x509_certificate(der) {
            anchors.push(cert);
        }
    }
    if anchors.is_empty() {
        return Err(VerificationError::UntrustedSigner);
    }

    // (2) Cryptographic chain check: the leaf must be signed by one of the
    // bundle's CAs (real signature verification, not structural checks).
    let mut signer = None;
    for anchor in &anchors {
        if leaf.verify_signature(Some(&anchor.subject_pki)).is_ok() {
            signer = Some(anchor);
            break;
        }
    }
    let signer = signer.ok_or(VerificationError::UntrustedSigner)?;

    // (3) Validity windows — enforced on the leaf and on the signing CA.
    let now_ts = system_time_to_unix(now);
    if now_ts < leaf.validity.not_before.timestamp() {
        return Err(VerificationError::NotYetValid(
            leaf.validity.not_before.timestamp(),
        ));
    }
    if now_ts > leaf.validity.not_after.timestamp() {
        return Err(VerificationError::Expired(
            leaf.validity.not_after.timestamp(),
        ));
    }
    if now_ts < signer.validity.not_before.timestamp()
        || now_ts > signer.validity.not_after.timestamp()
    {
        return Err(VerificationError::CaValidity);
    }

    // (4) Exactly one URI SAN, no other name forms.
    let spiffe_id = extract_id_from_leaf(&leaf)?;

    Ok(ChainVerified {
        spiffe_id,
        // Canonical big-integer hex — the SAME form issuance stamps on
        // IssuedSvid::serial (see canonical_serial_hex).
        serial: crate::canonical_serial_hex(&leaf.serial.to_bytes_be()),
        not_before: leaf.validity.not_before.timestamp(),
        not_after: leaf.validity.not_after.timestamp(),
    })
}

/// Check (4) alone: exactly one URI SAN carrying a well-formed SPIFFE ID,
/// and no other SAN name forms (per the SPIFFE X.509-SVID spec).
fn extract_id_from_leaf(leaf: &X509Certificate) -> Result<SpiffeId, VerificationError> {
    let mut uri_count = 0;
    let mut other_forms = 0;
    let mut spiffe_id_str: Option<String> = None;
    for ext in leaf.extensions() {
        if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
            for name in &san.general_names {
                match name {
                    GeneralName::URI(s) => {
                        uri_count += 1;
                        spiffe_id_str = Some(s.to_string());
                    }
                    _ => other_forms += 1,
                }
            }
        }
    }
    if other_forms > 0 {
        return Err(VerificationError::ForbiddenSanForm);
    }
    match uri_count {
        0 => return Err(VerificationError::NoUriSan),
        1 => {}
        n => return Err(VerificationError::MultipleUriSans(n)),
    }
    SpiffeId::parse(&spiffe_id_str.expect("uri_count == 1 guarantees the SAN string"))
        .map_err(|e| VerificationError::MalformedSpiffeId(e.to_string()))
}

/// Check (5): bind the SPIFFE ID to the expected agent session.
fn bind_to_expected(
    chain: ChainVerified,
    expected: &ExpectedIdentity,
) -> Result<VerifiedSvid, VerificationError> {
    let spiffe_id = chain.spiffe_id;
    if spiffe_id.trust_domain != expected.trust_domain {
        return Err(VerificationError::ForeignTrustDomain {
            found: spiffe_id.trust_domain,
            expected: expected.trust_domain.clone(),
        });
    }
    let agent_path = spiffe_id
        .agent_path()
        .ok_or(VerificationError::NotAgentPath)?;
    if agent_path.session_id != expected.session_id {
        return Err(VerificationError::WrongSession {
            found: agent_path.session_id,
            expected: expected.session_id.clone(),
        });
    }
    if agent_path.role != expected.role {
        return Err(VerificationError::WrongRole {
            found: agent_path.role,
            expected: expected.role.clone(),
        });
    }
    Ok(VerifiedSvid {
        spiffe_id,
        agent_path,
        serial: chain.serial,
        not_before: chain.not_before,
        not_after: chain.not_after,
    })
}

/// The result of checks (1)-(4): a cryptographically chain-verified,
/// in-validity-window SVID with a single well-formed URI SAN.
struct ChainVerified {
    spiffe_id: SpiffeId,
    serial: String,
    not_before: i64,
    not_after: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a PEM chain into DER blocks (leaf first). `None` when the input
/// contains no complete CERTIFICATE blocks.
fn parse_pem_chain(pem_text: &str) -> Option<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    // `parse_many` fails the whole input on any malformed block — a
    // presented SVID with a corrupt block is rejected, not partially
    // accepted (fail closed).
    let blocks = pem::parse_many(pem_text).ok()?;
    for pem in blocks {
        if pem.tag() == CHAIN_PEM_LABEL {
            out.push(pem.contents().to_vec());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn system_time_to_unix(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ca::TrustDomainCa;
    use time::OffsetDateTime;

    const TD: &str = "gyre.example.com";

    fn setup() -> (TrustDomainCa, SpiffeId, IssuedBundle) {
        let ca = TrustDomainCa::generate(TD).unwrap();
        let id = SpiffeId::agent(TD, "worker", "session-42").unwrap();
        let svid = ca.issue_svid(&id, 3600).unwrap();
        let expected = ExpectedIdentity {
            trust_domain: TD.to_string(),
            role: "worker".to_string(),
            session_id: "session-42".to_string(),
        };
        (
            ca,
            id,
            IssuedBundle {
                chain_pem: svid.chain_pem,
                expected,
                not_before: svid.not_before,
                not_after: svid.not_after,
            },
        )
    }

    struct IssuedBundle {
        chain_pem: String,
        expected: ExpectedIdentity,
        not_before: i64,
        not_after: i64,
    }

    fn now() -> SystemTime {
        SystemTime::now()
    }

    #[test]
    fn accepts_genuine_svid() {
        let (ca, id, issued) = setup();
        assert!(
            issued.not_after > issued.not_before,
            "issued SVID must carry a positive validity window"
        );
        let anchors = vec![ca.cert_der().to_vec()];
        let verified = verify_svid(&issued.chain_pem, &anchors, &issued.expected, now()).unwrap();
        assert_eq!(verified.spiffe_id, id);
        assert_eq!(verified.agent_path.session_id, "session-42");
    }

    #[test]
    fn rejects_no_svid() {
        let (ca, _, issued) = setup();
        let anchors = vec![ca.cert_der().to_vec()];
        let err = verify_svid("", &anchors, &issued.expected, now()).unwrap_err();
        assert_eq!(err, VerificationError::NoSvid);
        let err = verify_svid("   ", &anchors, &issued.expected, now()).unwrap_err();
        assert_eq!(err, VerificationError::NoSvid);
    }

    #[test]
    fn rejects_garbage_pem() {
        let (ca, _, issued) = setup();
        let anchors = vec![ca.cert_der().to_vec()];
        for garbage in ["not a pem", "-----BEGIN CERTIFICATE-----\nAAAA"] {
            let err = verify_svid(garbage, &anchors, &issued.expected, now()).unwrap_err();
            assert_eq!(err, VerificationError::MalformedPem, "{garbage:?}");
        }
    }

    /// The task's core probe: an SVID signed by a key NOT in the trust
    /// bundle, carrying a well-formed SPIFFE ID for the same session,
    /// must be rejected (chain validation, not metadata checks).
    #[test]
    fn rejects_svid_signed_by_unknown_key() {
        let (_ca, _, issued) = setup();
        let rogue = TrustDomainCa::generate(TD).unwrap();
        let rogue_id = SpiffeId::agent(TD, "worker", "session-42").unwrap();
        let rogue_svid = rogue.issue_svid(&rogue_id, 3600).unwrap();
        // Present the rogue SVID against the honest trust bundle.
        let anchors = vec![_ca.cert_der().to_vec()];
        let err =
            verify_svid(&rogue_svid.chain_pem, &anchors, &issued.expected, now()).unwrap_err();
        assert_eq!(err, VerificationError::UntrustedSigner);
    }

    #[test]
    fn rejects_expired_svid() {
        let (ca, id, _) = setup();
        let not_before = OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap();
        let not_after = OffsetDateTime::from_unix_timestamp(1_000_001_000).unwrap();
        let expired = ca
            .issue_svid_with_validity(&id, not_before, not_after)
            .unwrap();
        let anchors = vec![ca.cert_der().to_vec()];
        let expected = ExpectedIdentity {
            trust_domain: TD.to_string(),
            role: "worker".to_string(),
            session_id: "session-42".to_string(),
        };
        let err = verify_svid(&expired.chain_pem, &anchors, &expected, now()).unwrap_err();
        assert_eq!(err, VerificationError::Expired(not_after.unix_timestamp()));
    }

    #[test]
    fn rejects_not_yet_valid_svid() {
        let (ca, id, _) = setup();
        let not_before = OffsetDateTime::now_utc() + time::Duration::days(1);
        let not_after = OffsetDateTime::now_utc() + time::Duration::days(2);
        let future = ca
            .issue_svid_with_validity(&id, not_before, not_after)
            .unwrap();
        let anchors = vec![ca.cert_der().to_vec()];
        let expected = ExpectedIdentity {
            trust_domain: TD.to_string(),
            role: "worker".to_string(),
            session_id: "session-42".to_string(),
        };
        let err = verify_svid(&future.chain_pem, &anchors, &expected, now()).unwrap_err();
        assert_eq!(
            err,
            VerificationError::NotYetValid(not_before.unix_timestamp())
        );
    }

    #[test]
    fn rejects_foreign_trust_domain() {
        let (ca, _, issued) = setup();
        // A genuine SVID from OUR CA but naming a different trust domain
        // cannot exist (issuance binds the id) — instead verify a genuine
        // SVID against an expected identity whose trust domain differs.
        let foreign_expected = ExpectedIdentity {
            trust_domain: "other.example.org".to_string(),
            role: "worker".to_string(),
            session_id: "session-42".to_string(),
        };
        let anchors = vec![ca.cert_der().to_vec()];
        let err = verify_svid(&issued.chain_pem, &anchors, &foreign_expected, now()).unwrap_err();
        assert_eq!(
            err,
            VerificationError::ForeignTrustDomain {
                found: TD.to_string(),
                expected: "other.example.org".to_string(),
            }
        );
    }

    #[test]
    fn rejects_svid_for_different_session() {
        let (ca, _, issued) = setup();
        // Another genuine SVID from the same CA for a DIFFERENT session.
        let other_id = SpiffeId::agent(TD, "worker", "session-99").unwrap();
        let other = ca.issue_svid(&other_id, 3600).unwrap();
        let anchors = vec![ca.cert_der().to_vec()];
        let err = verify_svid(&other.chain_pem, &anchors, &issued.expected, now()).unwrap_err();
        assert_eq!(
            err,
            VerificationError::WrongSession {
                found: "session-99".to_string(),
                expected: "session-42".to_string(),
            }
        );
    }

    #[test]
    fn rejects_svid_for_different_role() {
        let (ca, _, issued) = setup();
        let other_id = SpiffeId::agent(TD, "security", "session-42").unwrap();
        let other = ca.issue_svid(&other_id, 3600).unwrap();
        let anchors = vec![ca.cert_der().to_vec()];
        let err = verify_svid(&other.chain_pem, &anchors, &issued.expected, now()).unwrap_err();
        assert_eq!(
            err,
            VerificationError::WrongRole {
                found: "security".to_string(),
                expected: "worker".to_string(),
            }
        );
    }

    #[test]
    fn rejects_when_bundle_has_no_anchors() {
        let (_, _, issued) = setup();
        let err = verify_svid(&issued.chain_pem, &[], &issued.expected, now()).unwrap_err();
        assert_eq!(err, VerificationError::UntrustedSigner);
    }

    /// A tampered leaf (bit-flipped DER) must not verify — the signature
    /// check covers the whole TBS, not just the SAN.
    #[test]
    fn rejects_tampered_leaf_bytes() {
        let (ca, _, issued) = setup();
        let anchors = vec![ca.cert_der().to_vec()];
        // Flip a byte inside the leaf DER (in the SAN region).
        let mut blocks = pem::parse_many(&issued.chain_pem).unwrap();
        let mut leaf_der = blocks.remove(0).contents().to_vec();
        let idx = leaf_der.len() / 2;
        leaf_der[idx] ^= 0xff;
        let tampered_pem = format!(
            "{}{}",
            pem::encode(&pem::Pem::new("CERTIFICATE", leaf_der)),
            blocks.iter().map(pem::encode).collect::<String>()
        );
        let result = verify_svid(&tampered_pem, &anchors, &issued.expected, now());
        assert!(result.is_err(), "tampered leaf must not verify");
    }

    /// Federation helper: extract_spiffe_id reads the trust domain (and full
    /// SPIFFE ID) off the leaf URI SAN WITHOUT any anchor — the caller then
    /// selects the exchanged bundle to verify against. It must reject the
    /// same malformed shapes the full verification rejects.
    #[test]
    fn extract_spiffe_id_reads_unverified_id() {
        let (ca, id, issued) = setup();
        let extracted = extract_spiffe_id(&issued.chain_pem).unwrap();
        assert_eq!(extracted, id);
        assert_eq!(extracted.trust_domain, TD);

        // No SVID / garbage / non-PEM inputs are rejected.
        assert_eq!(
            extract_spiffe_id("").unwrap_err(),
            VerificationError::NoSvid
        );
        assert_eq!(
            extract_spiffe_id("not a pem").unwrap_err(),
            VerificationError::MalformedPem
        );

        // The extracted ID is UNVERIFIED: an SVID signed by a rogue CA still
        // yields its (claimed) SPIFFE ID here — the anchor check happens in
        // verify_svid, not here.
        let rogue = TrustDomainCa::generate(TD).unwrap();
        let rogue_id = SpiffeId::agent(TD, "worker", "session-42").unwrap();
        let rogue_svid = rogue.issue_svid(&rogue_id, 3600).unwrap();
        assert_eq!(extract_spiffe_id(&rogue_svid.chain_pem).unwrap(), rogue_id);
        let _ = ca; // silences unused-variable when assertions short-circuit
    }
}
