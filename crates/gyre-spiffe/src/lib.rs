//! SPIFFE workload attestation for Gyre (identity-security.md Layer 1).
//!
//! Layer 1 of the identity stack proves \"this workload is real\" using the
//! specced existing protocol — SPIFFE X.509-SVIDs — instead of a bespoke
//! local scheme:
//!
//! - [`TrustDomainCa`] — the CA Gyre operates for its trust domain when no
//!   external SPIRE server / Workload API is configured. Issues
//!   SPIFFE-conformant X.509-SVIDs whose leaf certificate carries the
//!   workload's SPIFFE ID as a URI SAN in the exact spec format
//!   `spiffe://{trust-domain}/agent/{role}/session/{id}`
//!   (identity-security.md Layer 1).
//! - [`verify_svid`] — cryptographic verification of a presented SVID:
//!   leaf signature validated against the trust bundle's CA keys, validity
//!   periods enforced, and the SPIFFE ID parsed and bound to the expected
//!   agent session. This replaces the retired G10 `/proc/{pid}` probe and
//!   skip-on-empty string compare as the \"workload is real\" proof.
//! - [`trust_bundle_jwks`] — the trust bundle in SPIFFE bundle format (JWK
//!   Set with `use: "svid"`), served at the server's well-known endpoint so
//!   any verifier can check SVIDs.
//! - [`workload_api`] — a SPIFFE Workload API (gRPC over UDS) client used
//!   to obtain SVIDs from an externally configured SPIRE server.
//!
//! X.509-SVID conformance notes (SPIFFE X.509-SVID specification):
//! leaf signing keys and CA keys are ECDSA P-256 (the spec permits ECDSA
//! or RSA; Ed25519 is not an allowed SVID signing algorithm), the SPIFFE
//! ID is carried as the sole URI SAN, and SVIDs are short-lived.

pub mod bundle;
pub mod ca;
pub mod federation;
pub mod id;
pub mod verify;
pub mod workload_api;

pub use bundle::{parse_trust_bundle, trust_bundle_jwks};
pub use ca::{TrustDomainCa, BASE_SVID_TTL_SECS};
pub use id::{agent_spiffe_id, AgentPath, SpiffeId};
pub use verify::{
    extract_spiffe_id, verify_svid, verify_svid_for_record_id, ExpectedIdentity, VerificationError,
    VerifiedSvid, CHAIN_PEM_LABEL,
};

/// The canonical hex form of an X.509 serial number: minimal big-endian
/// hex with leading zero nibbles stripped (the big-integer `{:x}` form).
///
/// Issuance ([`ca::IssuedSvid::serial`]), verification
/// ([`verify::VerifiedSvid::serial`]), and the server-side record parse
/// all format serials through this one function. A serial whose top byte
/// is `0x01..=0x0F` carries a leading zero nibble in fixed-width hex that
/// the big-integer form strips — formatting the same serial differently
/// on the two sides of the issuance/verification comparison rejected
/// genuinely issued SVIDs as serial mismatches (task-261 review finding).
pub fn canonical_serial_hex(bytes: &[u8]) -> String {
    let full = hex::encode(bytes);
    let trimmed = full.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_serial_hex_strips_leading_zero_nibbles() {
        // Top byte 0x01..=0x0F: fixed-width hex carries one leading zero
        // nibble; the canonical (big-integer) form must strip it.
        assert_eq!(canonical_serial_hex(&[0x01, 0xab, 0xcd]), "1abcd");
        assert_eq!(canonical_serial_hex(&[0x0f, 0x00]), "f00");
        // Top byte >= 0x10: no leading zero nibble, identical to plain hex.
        assert_eq!(canonical_serial_hex(&[0x10, 0x20]), "1020");
        assert_eq!(canonical_serial_hex(&[0xff]), "ff");
        // Degenerate all-zero serial formats as "0", never "".
        assert_eq!(canonical_serial_hex(&[0x00]), "0");
    }
}
