//! ConversationRepository port — conversation-to-code provenance (HSI §5).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{Id, TurnCommitLink};

/// Repository for storing and querying agent conversation provenance.
///
/// Adapters implement encryption at rest and large-blob disk storage internally.
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    /// Store a conversation blob with metadata. Returns the SHA-256 hash.
    ///
    /// The `conversation` bytes are the raw zstd-compressed bytes (after base64 decode).
    /// The adapter stores them encrypted at rest.
    async fn store(
        &self,
        agent_id: &Id,
        workspace_id: &Id,
        tenant_id: &Id,
        conversation: &[u8],
    ) -> Result<String>;

    /// Retrieve a conversation by SHA. Returns decompressed bytes.
    ///
    /// The adapter handles decryption and decompression internally.
    /// Verifies that `tenant_id` matches the caller's tenant.
    async fn get(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<Vec<u8>>>;

    /// Record a turn-to-commit link (called from git push handler).
    async fn record_turn_link(&self, link: &TurnCommitLink) -> Result<()>;

    /// Get turn-commit links for a conversation (for "View conversation at this point").
    ///
    /// `tenant_id` ensures cross-tenant isolation on turn link queries.
    async fn get_turn_links(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Vec<TurnCommitLink>>;

    /// Get metadata for a conversation without fetching the full blob.
    ///
    /// Used by per-handler auth to resolve `workspace_id` from a SHA.
    /// Returns `(agent_id, workspace_id)` or `None` if not found.
    async fn get_metadata(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Option<(Id, Id)>>;

    /// List conversations for an agent (for Explorer detail panel provenance links).
    ///
    /// Returns SHAs ordered by `created_at` descending.
    async fn list_by_agent(&self, agent_id: &Id, tenant_id: &Id) -> Result<Vec<String>>;

    /// Back-fill `conversation_sha` on all `TurnCommitLink` records for an agent.
    ///
    /// Called when the conversation is uploaded at completion time.
    async fn backfill_turn_links(
        &self,
        agent_id: &Id,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<u64>;
}
