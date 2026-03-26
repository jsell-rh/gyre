//! PostgreSQL stub for ConversationRepository (HSI §5).
//!
//! NOTE: Conversation blob storage uses BLOB/BYTEA which requires different
//! migration SQL for PostgreSQL. This stub returns errors until a PG-specific
//! migration is added. For production PG deployments, replace this with a
//! proper BYTEA implementation.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use gyre_common::{Id, TurnCommitLink};
use gyre_ports::ConversationRepository;

use super::PgStorage;

#[async_trait]
impl ConversationRepository for PgStorage {
    async fn store(
        &self,
        _agent_id: &Id,
        _workspace_id: &Id,
        _tenant_id: &Id,
        _conversation: &[u8],
    ) -> Result<String> {
        Err(anyhow!(
            "ConversationRepository not yet implemented for PostgreSQL; use SQLite or add PG migration"
        ))
    }

    async fn get(&self, _conversation_sha: &str, _tenant_id: &Id) -> Result<Option<Vec<u8>>> {
        Err(anyhow!(
            "ConversationRepository not yet implemented for PostgreSQL"
        ))
    }

    async fn record_turn_link(&self, _link: &TurnCommitLink) -> Result<()> {
        Err(anyhow!(
            "ConversationRepository not yet implemented for PostgreSQL"
        ))
    }

    async fn get_turn_links(
        &self,
        _conversation_sha: &str,
        _tenant_id: &Id,
    ) -> Result<Vec<TurnCommitLink>> {
        Ok(vec![])
    }

    async fn get_metadata(
        &self,
        _conversation_sha: &str,
        _tenant_id: &Id,
    ) -> Result<Option<(Id, Id)>> {
        Ok(None)
    }

    async fn list_by_agent(&self, _agent_id: &Id, _tenant_id: &Id) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn backfill_turn_links(
        &self,
        _agent_id: &Id,
        _conversation_sha: &str,
        _tenant_id: &Id,
    ) -> Result<u64> {
        Ok(0)
    }
}
