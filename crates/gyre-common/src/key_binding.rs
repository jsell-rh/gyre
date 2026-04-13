//! Ephemeral key binding types (authorization-provenance.md §2.3).
//!
//! A key binding ties an ephemeral Ed25519 keypair to a user or workload identity,
//! authenticated by a trust anchor. The platform countersigns as a timestamp witness,
//! but is NOT a trust root.

use serde::{Deserialize, Serialize};

/// An ephemeral Ed25519 keypair bound to a user or agent identity (§2.3).
///
/// Key lifecycle:
/// 1. User authenticates via tenant IdP.
/// 2. Client generates an ephemeral Ed25519 keypair.
/// 3. Client constructs this document and signs it with the private key.
/// 4. Platform verifies IdP session, countersigns, and stores the public key.
/// 5. Private key remains client-side. It signs `SignedInput` documents.
/// 6. On expiry or logout, the binding is invalidated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Ed25519 public key bytes.
    pub public_key: Vec<u8>,
    /// Subject claim from IdP JWT (e.g., "user:jsell") or agent JWT sub claim.
    pub user_identity: String,
    /// IdP issuer URL or Gyre OIDC issuer URL.
    pub issuer: String,
    /// Which TrustAnchor authenticated this identity.
    pub trust_anchor_id: String,
    /// When this binding was created (Unix epoch seconds).
    pub issued_at: u64,
    /// When this binding expires (Unix epoch seconds).
    pub expires_at: u64,
    /// User signs this binding document with the ephemeral key.
    pub user_signature: Vec<u8>,
    /// Platform countersigns (proves binding was registered at a specific time
    /// with a valid IdP session). This is a timestamp witness, not an authority delegation.
    pub platform_countersign: Vec<u8>,
}

impl KeyBinding {
    /// Returns true if this binding has expired relative to the given timestamp.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key_binding() -> KeyBinding {
        KeyBinding {
            public_key: vec![1, 2, 3, 4, 5, 6, 7, 8],
            user_identity: "user:jsell".to_string(),
            issuer: "https://keycloak.example.com".to_string(),
            trust_anchor_id: "tenant-keycloak".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_003_600,
            user_signature: vec![10, 20, 30, 40],
            platform_countersign: vec![50, 60, 70, 80],
        }
    }

    #[test]
    fn key_binding_roundtrip() {
        let kb = sample_key_binding();
        let json = serde_json::to_string(&kb).unwrap();
        let back: KeyBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kb);
    }

    #[test]
    fn key_binding_all_fields_present_in_json() {
        let kb = sample_key_binding();
        let json = serde_json::to_string(&kb).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("public_key").is_some());
        assert!(v.get("user_identity").is_some());
        assert!(v.get("issuer").is_some());
        assert!(v.get("trust_anchor_id").is_some());
        assert!(v.get("issued_at").is_some());
        assert!(v.get("expires_at").is_some());
        assert!(v.get("user_signature").is_some());
        assert!(v.get("platform_countersign").is_some());
    }

    #[test]
    fn is_expired_before_expiry() {
        let kb = sample_key_binding();
        assert!(!kb.is_expired(1_700_000_000));
        assert!(!kb.is_expired(1_700_003_599));
    }

    #[test]
    fn is_expired_at_and_after_expiry() {
        let kb = sample_key_binding();
        assert!(kb.is_expired(1_700_003_600));
        assert!(kb.is_expired(1_700_010_000));
    }

    #[test]
    fn key_binding_agent_identity_roundtrip() {
        let kb = KeyBinding {
            public_key: vec![9, 8, 7],
            user_identity: "agent:orchestrator-1".to_string(),
            issuer: "https://gyre.example.com".to_string(),
            trust_anchor_id: "gyre-oidc".to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_001_800,
            user_signature: vec![1, 2],
            platform_countersign: vec![3, 4],
        };
        let json = serde_json::to_string(&kb).unwrap();
        let back: KeyBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(back.user_identity, "agent:orchestrator-1");
        assert_eq!(back.trust_anchor_id, "gyre-oidc");
    }
}
