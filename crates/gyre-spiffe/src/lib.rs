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
pub mod id;
pub mod verify;
pub mod workload_api;

pub use bundle::{parse_trust_bundle, trust_bundle_jwks};
pub use ca::{TrustDomainCa, BASE_SVID_TTL_SECS};
pub use id::{agent_spiffe_id, AgentPath, SpiffeId};
pub use verify::{
    verify_svid, verify_svid_for_record_id, ExpectedIdentity, VerificationError, VerifiedSvid,
    CHAIN_PEM_LABEL,
};
