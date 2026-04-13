//! Stub PgStorage implementations for authorization provenance port traits.
//! Full implementation deferred to future milestone. SQLite is the primary backend.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Attestation, KeyBinding, TrustAnchor};
use gyre_ports::{ChainAttestationRepository, KeyBindingRepository, TrustAnchorRepository};

use super::PgStorage;

#[async_trait]
impl ChainAttestationRepository for PgStorage {
    async fn save(&self, _attestation: &Attestation) -> Result<()> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }

    async fn find_by_id(&self, _id: &str) -> Result<Option<Attestation>> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }

    async fn load_chain(&self, _leaf_id: &str) -> Result<Vec<Attestation>> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }

    async fn find_by_task(&self, _task_id: &str) -> Result<Vec<Attestation>> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }

    async fn find_by_commit(&self, _commit_sha: &str) -> Result<Option<Attestation>> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }

    async fn find_by_repo(
        &self,
        _repo_id: &str,
        _since: u64,
        _until: u64,
    ) -> Result<Vec<Attestation>> {
        anyhow::bail!("ChainAttestationRepository not implemented for PgStorage")
    }
}

#[async_trait]
impl KeyBindingRepository for PgStorage {
    async fn store(&self, _tenant_id: &str, _binding: &KeyBinding) -> Result<()> {
        anyhow::bail!("KeyBindingRepository not implemented for PgStorage")
    }

    async fn find_by_public_key(
        &self,
        _tenant_id: &str,
        _public_key: &[u8],
    ) -> Result<Option<KeyBinding>> {
        anyhow::bail!("KeyBindingRepository not implemented for PgStorage")
    }

    async fn find_active_by_identity(
        &self,
        _tenant_id: &str,
        _user_identity: &str,
    ) -> Result<Vec<KeyBinding>> {
        anyhow::bail!("KeyBindingRepository not implemented for PgStorage")
    }

    async fn invalidate(&self, _tenant_id: &str, _public_key: &[u8]) -> Result<()> {
        anyhow::bail!("KeyBindingRepository not implemented for PgStorage")
    }

    async fn invalidate_all_for_identity(
        &self,
        _tenant_id: &str,
        _user_identity: &str,
    ) -> Result<()> {
        anyhow::bail!("KeyBindingRepository not implemented for PgStorage")
    }
}

#[async_trait]
impl TrustAnchorRepository for PgStorage {
    async fn create(&self, _tenant_id: &str, _anchor: &TrustAnchor) -> Result<()> {
        anyhow::bail!("TrustAnchorRepository not implemented for PgStorage")
    }

    async fn find_by_id(&self, _tenant_id: &str, _anchor_id: &str) -> Result<Option<TrustAnchor>> {
        anyhow::bail!("TrustAnchorRepository not implemented for PgStorage")
    }

    async fn list_by_tenant(&self, _tenant_id: &str) -> Result<Vec<TrustAnchor>> {
        anyhow::bail!("TrustAnchorRepository not implemented for PgStorage")
    }

    async fn update(&self, _tenant_id: &str, _anchor: &TrustAnchor) -> Result<()> {
        anyhow::bail!("TrustAnchorRepository not implemented for PgStorage")
    }

    async fn delete(&self, _tenant_id: &str, _anchor_id: &str) -> Result<()> {
        anyhow::bail!("TrustAnchorRepository not implemented for PgStorage")
    }
}
