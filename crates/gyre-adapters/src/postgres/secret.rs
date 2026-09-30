//! Stub PgStorage implementation for the SecretRepository port.
//! Full implementation deferred; SQLite is the primary backend.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Id, Secret, SecretScope};
use gyre_ports::SecretRepository;

use super::PgStorage;

#[async_trait]
impl SecretRepository for PgStorage {
    async fn create(&self, _secret: &Secret, _value: &[u8]) -> Result<()> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }

    async fn get_value(&self, _id: &Id, _tenant_id: &str) -> Result<Option<Vec<u8>>> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }

    async fn list_by_scope(
        &self,
        _scope: SecretScope,
        _scope_id: &str,
        _tenant_id: &str,
    ) -> Result<Vec<Secret>> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }

    async fn delete(&self, _id: &Id, _tenant_id: &str) -> Result<()> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }

    async fn rotate(&self, _id: &Id, _new_value: &[u8], _tenant_id: &str) -> Result<()> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }

    async fn resolve_for_agent(
        &self,
        _tenant_id: &str,
        _workspace_id: &str,
        _repo_id: &str,
        _task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        anyhow::bail!("SecretRepository not implemented for PgStorage")
    }
}
