use anyhow::Result;
use async_trait::async_trait;

/// A single step recorded during agent spawn or complete (M13.7).
pub struct SpawnLogEntry {
    pub agent_id: String,
    pub step: String,
    pub status: String,
    pub detail: Option<String>,
    pub occurred_at: u64,
}

/// Repository for agent spawn log entries and revoked tokens (M13.7).
#[async_trait]
pub trait SpawnLogRepository: Send + Sync {
    /// Append a step to the spawn log for an agent.
    async fn append_spawn_step(
        &self,
        agent_id: &str,
        step: &str,
        status: &str,
        detail: Option<&str>,
        occurred_at: u64,
    ) -> Result<()>;

    /// Get all spawn log entries for an agent, ordered by occurred_at ascending.
    async fn get_spawn_log(&self, agent_id: &str) -> Result<Vec<SpawnLogEntry>>;

    /// Record a revoked token (SHA-256 hash of the raw token).
    async fn revoke_token(&self, token_hash: &str, agent_id: &str, revoked_at: u64) -> Result<()>;

    /// Check whether a token hash has been revoked.
    async fn is_token_revoked(&self, token_hash: &str) -> Result<bool>;
}
