use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{message::Message, Id};

/// Port trait for message persistence (Directed and Event tier).
/// Telemetry-tier messages do NOT go through this trait — they use TelemetryBuffer.
///
/// Tenant isolation is enforced structurally: workspace IDs are globally unique and
/// every workspace belongs to exactly one tenant. Querying by workspace_id implicitly
/// isolates by tenant.
#[async_trait]
pub trait MessageRepository: Send + Sync {
    /// Store a signed message (Directed or Event tier).
    async fn store(&self, message: &Message) -> Result<()>;

    /// Find a message by ID.
    async fn find_by_id(&self, id: &Id) -> Result<Option<Message>>;

    /// List Directed messages for an agent after a composite cursor, oldest first.
    ///
    /// When `after_id` is absent: `WHERE created_at > after_ts ORDER BY created_at, id LIMIT limit`.
    /// When `after_id` is present: `WHERE (created_at, id) > (after_ts, after_id)`.
    /// First poll: `after_ts=0, after_id=None`. Both paths use strict `>` — no duplicates.
    async fn list_after(
        &self,
        agent_id: &Id,
        after_ts: u64,
        after_id: Option<&Id>,
        limit: usize,
    ) -> Result<Vec<Message>>;

    /// List unacknowledged Directed messages for an agent (crash recovery), oldest first.
    async fn list_unacked(&self, agent_id: &Id, limit: usize) -> Result<Vec<Message>>;

    /// Count unacknowledged Directed messages for an agent (for limit enforcement).
    async fn count_unacked(&self, agent_id: &Id) -> Result<u64>;

    /// Acknowledge a message. Idempotent — acking an already-acked message returns Ok(()).
    async fn acknowledge(&self, message_id: &Id, agent_id: &Id) -> Result<()>;

    /// Bulk-acknowledge all messages for an agent (on agent completion).
    /// Returns the count of messages acknowledged.
    async fn acknowledge_all(&self, agent_id: &Id, reason: &str) -> Result<u64>;

    /// List messages in a workspace, optionally filtered by kind.
    ///
    /// Windowed query: `since` is a lower bound (filter, not cursor).
    /// `before_ts`/`before_id` is the pagination cursor (upper bound). Results ordered newest first.
    /// Omitting `since` returns all messages up to `before`.
    /// Omitting `before` returns the newest `limit` messages after `since`.
    async fn list_by_workspace(
        &self,
        workspace_id: &Id,
        kind: Option<&str>,
        since: Option<u64>,
        before_ts: Option<u64>,
        before_id: Option<&Id>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>>;

    /// Delete non-agent-targeted messages older than the given epoch milliseconds.
    ///
    /// Relies on invariant: Directed-tier messages always have to_type = 'agent',
    /// so filtering on to_type != 'agent' only removes Event-tier workspace/broadcast messages.
    /// Returns count deleted.
    async fn expire_events(&self, older_than: u64) -> Result<u64>;

    /// Delete Directed messages for dead agents older than the given epoch milliseconds.
    ///
    /// Matches messages where ack_reason IN ('agent_completed', 'agent_orphaned').
    /// Returns count deleted.
    async fn expire_acked_inboxes(&self, older_than: u64) -> Result<u64>;

    /// Delete unacked Directed messages for specific dead agent IDs.
    ///
    /// The server layer (not the repository) determines which agents are dead
    /// by querying AgentRepository, then passes the IDs here. This preserves
    /// port isolation — MessageRepository does not depend on AgentRepository.
    /// Returns count deleted.
    async fn expire_for_agents(&self, agent_ids: &[Id], older_than: u64) -> Result<u64>;
}
