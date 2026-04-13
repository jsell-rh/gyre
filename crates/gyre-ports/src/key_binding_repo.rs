//! Port trait for ephemeral key binding persistence (authorization-provenance.md §2.3).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::KeyBinding;

/// Repository for storing, querying, and invalidating ephemeral key bindings.
///
/// Key bindings tie ephemeral Ed25519 keypairs to user or agent identities.
/// They are short-lived (expire with IdP session or agent JWT) and must be
/// invalidated on logout or revocation.
#[async_trait]
pub trait KeyBindingRepository: Send + Sync {
    /// Store a new key binding after platform countersignature.
    async fn store(&self, tenant_id: &str, binding: &KeyBinding) -> Result<()>;

    /// Find a key binding by public key bytes.
    async fn find_by_public_key(
        &self,
        tenant_id: &str,
        public_key: &[u8],
    ) -> Result<Option<KeyBinding>>;

    /// Find all active (non-expired, non-invalidated) key bindings for a user identity.
    async fn find_active_by_identity(
        &self,
        tenant_id: &str,
        user_identity: &str,
    ) -> Result<Vec<KeyBinding>>;

    /// Invalidate a key binding (logout, revocation, or expiry cleanup).
    /// After invalidation, the key binding will not be returned by `find_by_public_key`
    /// or `find_active_by_identity`.
    async fn invalidate(&self, tenant_id: &str, public_key: &[u8]) -> Result<()>;

    /// Invalidate all key bindings for a user identity (e.g., on logout-all).
    async fn invalidate_all_for_identity(&self, tenant_id: &str, user_identity: &str)
        -> Result<()>;
}
