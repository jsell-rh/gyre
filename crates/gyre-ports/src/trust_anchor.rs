//! Port trait for trust anchor persistence (authorization-provenance.md §1.1).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::TrustAnchor;

/// CRUD repository for trust anchors, scoped to a tenant.
///
/// Trust anchors are identity issuers (IdP, OIDC, addon) that the verification
/// algorithm trusts. They are tenant-scoped — a workspace inherits its tenant's
/// anchors. The platform cannot modify trust anchors without human admin action.
#[async_trait]
pub trait TrustAnchorRepository: Send + Sync {
    /// Register a new trust anchor for a tenant.
    async fn create(&self, tenant_id: &str, anchor: &TrustAnchor) -> Result<()>;

    /// Find a trust anchor by its stable identifier within a tenant.
    async fn find_by_id(&self, tenant_id: &str, anchor_id: &str) -> Result<Option<TrustAnchor>>;

    /// List all trust anchors for a tenant.
    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<TrustAnchor>>;

    /// Update an existing trust anchor (e.g., rotate JWKS URI).
    async fn update(&self, tenant_id: &str, anchor: &TrustAnchor) -> Result<()>;

    /// Remove a trust anchor. Existing attestations that reference this anchor
    /// will fail verification — this is intentional (revocation).
    async fn delete(&self, tenant_id: &str, anchor_id: &str) -> Result<()>;
}
