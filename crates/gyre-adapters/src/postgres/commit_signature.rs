//! Stub PgStorage implementation for the commit-signature port.
//! SQLite is the primary backend for this port (see sqlite/commit_signature.rs).

use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::commit_signature_repo::{CommitSignature, CommitSignatureRepository};

use super::PgStorage;

#[async_trait]
impl CommitSignatureRepository for PgStorage {
    async fn save(&self, _record: &CommitSignature) -> Result<()> {
        anyhow::bail!("CommitSignatureRepository not implemented for PgStorage")
    }

    async fn find(&self, _repo_id: &str, _commit_sha: &str) -> Result<Option<CommitSignature>> {
        anyhow::bail!("CommitSignatureRepository not implemented for PgStorage")
    }
}
