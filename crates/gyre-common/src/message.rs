use crate::Id;
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::VecDeque;

/// The unified message envelope — all inter-component communication flows through this type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: Id,
    pub tenant_id: Id,

    /// WHO sent this — derived server-side, never caller-supplied.
    pub from: MessageOrigin,

    /// Workspace scope — None only for Broadcast destination (never persisted).
    pub workspace_id: Option<Id>,

    /// WHO receives this.
    pub to: Destination,

    /// WHAT was said.
    pub kind: MessageKind,

    /// Structured data specific to the kind.
    pub payload: Option<Value>,

    /// Unix epoch MILLISECONDS for sub-second ordering.
    pub created_at: u64,

    /// Ed25519 signature — present on Directed + Event tier, absent on Telemetry.
    pub signature: Option<String>,

    /// Key ID from the server's JWKS.
    pub key_id: Option<String>,

    /// Delivery state — only meaningful for Directed tier with Destination::Agent.
    /// Excluded from send responses and WebSocket pushes; included in GET inbox responses.
    pub acknowledged: bool,
}

/// Identifies who sent a message. Serde: externally tagged, snake_case.
/// Wire: `"server"` (unit variant serializes as string), `{"agent": "<id>"}`, `{"user": "<id>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Server,
    Agent(Id),
    User(Id),
}

/// Identifies who receives a message. Serde: externally tagged, snake_case.
/// Wire: `{"agent": "<id>"}`, `{"workspace": "<id>"}`, `"broadcast"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Agent(Id),
    Workspace(Id),
    Broadcast,
}

/// Message tier — determines signing and persistence behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageTier {
    /// Signed + persisted until acked. Agent-to-agent commands.
    Directed,
    /// Signed + persisted with TTL. System state changes.
    Event,
    /// Unsigned + in-memory ring buffer only. High-frequency observability.
    Telemetry,
}

/// One enum replaces three current type systems (REST inbox, domain events, AG-UI telemetry).
///
/// Wire format: a plain snake_case string — e.g., "agent_created", "task_assignment".
/// Custom kinds use the raw string: Custom("my_event") → "my_event".
/// Unknown strings deserialize to Custom(s) via custom Deserialize.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageKind {
    // ── Tier 1: Directed (signed + persisted + ack-based) ──────────────
    TaskAssignment,
    ReviewRequest,
    StatusUpdate,
    Escalation,

    // ── Tier 2: Events (signed + persisted with TTL) ────────────────────
    AgentCreated,
    AgentStatusChanged,
    AgentContainerSpawned,
    /// Emitted by the server when an agent calls `agent.complete` (HSI §4).
    /// server_only = true. Destination: Workspace(agent's workspace_id).
    AgentCompleted,
    /// Emitted when a meta-spec reconciliation run completes (HSI §4 / meta-spec-reconciliation §11).
    /// server_only = true. Consumed by MessageConsumer to create priority-6 MetaSpecDrift notifications.
    ReconciliationCompleted,
    TaskCreated,
    TaskTransitioned,
    MrCreated,
    MrStatusChanged,
    MrMerged,
    QueueUpdated,
    PushRejected,
    PushAccepted,
    SpecChanged,
    /// Emitted when a human approves a spec (`POST /api/v1/specs/:path/approve`).
    /// server_only = true. Payload: `{repo_id, spec_path, spec_sha, approved_by, approval_id}`.
    /// Distinct from `SpecChanged` (which fires on push, before approval).
    /// Destination: Workspace(workspace_id) — consumed by workspace orchestrator.
    SpecApproved,
    GateFailure,
    StaleSpecWarning,
    SpeculativeConflict,
    SpeculativeMergeClean,
    HotFilesChanged,
    DataSeeded,
    BudgetWarning,
    BudgetExhausted,
    AgentError,
    /// Emitted when a constraint evaluation fails at push or merge time (§7.5).
    /// server_only = true. Tier: Event (signed, TTL).
    /// Broadcast to workspace and directed to the author agent.
    ConstraintViolation,

    // ── Tier 3: Telemetry (unsigned + in-memory only) ──────────────────
    ToolCallStart,
    ToolCallEnd,
    TextMessageContent,
    RunStarted,
    RunFinished,
    StateChanged,

    // ── Custom ──────────────────────────────────────────────────────────
    /// Unknown or extension kind strings. Defaults to Event tier.
    Custom(String),
}

impl MessageKind {
    /// Returns the snake_case wire name for this kind.
    pub fn as_str(&self) -> &str {
        match self {
            MessageKind::TaskAssignment => "task_assignment",
            MessageKind::ReviewRequest => "review_request",
            MessageKind::StatusUpdate => "status_update",
            MessageKind::Escalation => "escalation",
            MessageKind::AgentCreated => "agent_created",
            MessageKind::AgentStatusChanged => "agent_status_changed",
            MessageKind::AgentContainerSpawned => "agent_container_spawned",
            MessageKind::AgentCompleted => "agent_completed",
            MessageKind::ReconciliationCompleted => "reconciliation_completed",
            MessageKind::TaskCreated => "task_created",
            MessageKind::TaskTransitioned => "task_transitioned",
            MessageKind::MrCreated => "mr_created",
            MessageKind::MrStatusChanged => "mr_status_changed",
            MessageKind::MrMerged => "mr_merged",
            MessageKind::QueueUpdated => "queue_updated",
            MessageKind::PushRejected => "push_rejected",
            MessageKind::PushAccepted => "push_accepted",
            MessageKind::SpecChanged => "spec_changed",
            MessageKind::SpecApproved => "spec_approved",
            MessageKind::GateFailure => "gate_failure",
            MessageKind::StaleSpecWarning => "stale_spec_warning",
            MessageKind::SpeculativeConflict => "speculative_conflict",
            MessageKind::SpeculativeMergeClean => "speculative_merge_clean",
            MessageKind::HotFilesChanged => "hot_files_changed",
            MessageKind::DataSeeded => "data_seeded",
            MessageKind::BudgetWarning => "budget_warning",
            MessageKind::BudgetExhausted => "budget_exhausted",
            MessageKind::AgentError => "agent_error",
            MessageKind::ConstraintViolation => "constraint_violation",
            MessageKind::ToolCallStart => "tool_call_start",
            MessageKind::ToolCallEnd => "tool_call_end",
            MessageKind::TextMessageContent => "text_message_content",
            MessageKind::RunStarted => "run_started",
            MessageKind::RunFinished => "run_finished",
            MessageKind::StateChanged => "state_changed",
            MessageKind::Custom(s) => s.as_str(),
        }
    }

    /// Returns true for built-in Event-tier variants — these may only be sent by the server.
    /// Custom(String) returns false even though it defaults to Event tier.
    pub fn server_only(&self) -> bool {
        matches!(
            self,
            MessageKind::AgentCreated
                | MessageKind::AgentStatusChanged
                | MessageKind::AgentContainerSpawned
                | MessageKind::AgentCompleted
                | MessageKind::ReconciliationCompleted
                | MessageKind::TaskCreated
                | MessageKind::TaskTransitioned
                | MessageKind::MrCreated
                | MessageKind::MrStatusChanged
                | MessageKind::MrMerged
                | MessageKind::QueueUpdated
                | MessageKind::PushRejected
                | MessageKind::PushAccepted
                | MessageKind::SpecChanged
                | MessageKind::SpecApproved
                | MessageKind::GateFailure
                | MessageKind::StaleSpecWarning
                | MessageKind::SpeculativeConflict
                | MessageKind::SpeculativeMergeClean
                | MessageKind::HotFilesChanged
                | MessageKind::DataSeeded
                | MessageKind::BudgetWarning
                | MessageKind::BudgetExhausted
                | MessageKind::AgentError
                | MessageKind::ConstraintViolation
        )
    }

    /// Returns the tier for this kind.
    /// Custom defaults to Event tier.
    pub fn tier(&self) -> MessageTier {
        match self {
            MessageKind::TaskAssignment
            | MessageKind::ReviewRequest
            | MessageKind::StatusUpdate
            | MessageKind::Escalation => MessageTier::Directed,

            MessageKind::AgentCreated
            | MessageKind::AgentStatusChanged
            | MessageKind::AgentContainerSpawned
            | MessageKind::AgentCompleted
            | MessageKind::ReconciliationCompleted
            | MessageKind::TaskCreated
            | MessageKind::TaskTransitioned
            | MessageKind::MrCreated
            | MessageKind::MrStatusChanged
            | MessageKind::MrMerged
            | MessageKind::QueueUpdated
            | MessageKind::PushRejected
            | MessageKind::PushAccepted
            | MessageKind::SpecChanged
            | MessageKind::SpecApproved
            | MessageKind::GateFailure
            | MessageKind::StaleSpecWarning
            | MessageKind::SpeculativeConflict
            | MessageKind::SpeculativeMergeClean
            | MessageKind::HotFilesChanged
            | MessageKind::DataSeeded
            | MessageKind::BudgetWarning
            | MessageKind::BudgetExhausted
            | MessageKind::AgentError
            | MessageKind::ConstraintViolation
            | MessageKind::Custom(_) => MessageTier::Event,

            MessageKind::ToolCallStart
            | MessageKind::ToolCallEnd
            | MessageKind::TextMessageContent
            | MessageKind::RunStarted
            | MessageKind::RunFinished
            | MessageKind::StateChanged => MessageTier::Telemetry,
        }
    }

    /// Parse from a wire string, returning Custom(s) for unknown values.
    fn from_wire(s: &str) -> Self {
        match s {
            "task_assignment" => MessageKind::TaskAssignment,
            "review_request" => MessageKind::ReviewRequest,
            "status_update" => MessageKind::StatusUpdate,
            "escalation" => MessageKind::Escalation,
            "agent_created" => MessageKind::AgentCreated,
            "agent_status_changed" => MessageKind::AgentStatusChanged,
            "agent_container_spawned" => MessageKind::AgentContainerSpawned,
            "agent_completed" => MessageKind::AgentCompleted,
            "reconciliation_completed" => MessageKind::ReconciliationCompleted,
            "task_created" => MessageKind::TaskCreated,
            "task_transitioned" => MessageKind::TaskTransitioned,
            "mr_created" => MessageKind::MrCreated,
            "mr_status_changed" => MessageKind::MrStatusChanged,
            "mr_merged" => MessageKind::MrMerged,
            "queue_updated" => MessageKind::QueueUpdated,
            "push_rejected" => MessageKind::PushRejected,
            "push_accepted" => MessageKind::PushAccepted,
            "spec_changed" => MessageKind::SpecChanged,
            "spec_approved" => MessageKind::SpecApproved,
            "gate_failure" => MessageKind::GateFailure,
            "stale_spec_warning" => MessageKind::StaleSpecWarning,
            "speculative_conflict" => MessageKind::SpeculativeConflict,
            "speculative_merge_clean" => MessageKind::SpeculativeMergeClean,
            "hot_files_changed" => MessageKind::HotFilesChanged,
            "data_seeded" => MessageKind::DataSeeded,
            "budget_warning" => MessageKind::BudgetWarning,
            "budget_exhausted" => MessageKind::BudgetExhausted,
            "agent_error" => MessageKind::AgentError,
            "constraint_violation" => MessageKind::ConstraintViolation,
            "tool_call_start" => MessageKind::ToolCallStart,
            "tool_call_end" => MessageKind::ToolCallEnd,
            "text_message_content" => MessageKind::TextMessageContent,
            "run_started" => MessageKind::RunStarted,
            "run_finished" => MessageKind::RunFinished,
            "state_changed" => MessageKind::StateChanged,
            other => MessageKind::Custom(other.to_string()),
        }
    }
}

impl std::fmt::Display for MessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for MessageKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MessageKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(MessageKind::from_wire(&s))
    }
}

/// In-memory ring buffer for Telemetry-tier messages.
/// Keyed by workspace_id. Replaces the existing ActivityStore.
///
/// Per-workspace capacity: when a workspace's buffer exceeds `max_per_workspace`,
/// the oldest entry is evicted (FIFO ring buffer).
///
/// Per-server workspace capacity: when total distinct workspaces exceed `max_workspaces`,
/// the workspace with the most entries is evicted first (largest-first), per spec
/// §Telemetry Store. This prevents a tenant from flushing the buffer by creating many
/// empty workspaces.
#[derive(Debug)]
pub struct TelemetryBuffer {
    /// Per-workspace ring buffers. Key: workspace_id.
    buffers: DashMap<Id, VecDeque<Message>>,
    /// Maximum entries per workspace before oldest-entry eviction (default 10,000).
    max_per_workspace: usize,
    /// Maximum number of distinct workspaces before largest-workspace eviction (default 100).
    max_workspaces: usize,
}

impl TelemetryBuffer {
    pub fn new(max_per_workspace: usize, max_workspaces: usize) -> Self {
        assert!(max_per_workspace > 0, "max_per_workspace must be > 0");
        assert!(max_workspaces > 0, "max_workspaces must be > 0");
        Self {
            buffers: DashMap::new(),
            max_per_workspace,
            max_workspaces,
        }
    }

    /// Push a Telemetry-tier message into the workspace buffer.
    /// Non-Telemetry messages are silently ignored.
    /// Evicts the oldest entry per workspace when `max_per_workspace` is exceeded.
    /// Evicts the largest workspace when `max_workspaces` is exceeded (largest-first).
    pub fn push(&self, message: Message) {
        // Only Telemetry-tier messages belong in this buffer.
        if message.kind.tier() != MessageTier::Telemetry {
            return;
        }
        let workspace_id = match &message.workspace_id {
            Some(id) => id.clone(),
            None => return, // Telemetry requires a workspace scope
        };

        {
            let mut buf = self.buffers.entry(workspace_id).or_default();
            buf.push_back(message);
            if buf.len() > self.max_per_workspace {
                buf.pop_front();
            }
        } // release DashMap shard lock before workspace-count check

        // Enforce workspace count limit: evict the workspace with the most entries.
        if self.buffers.len() > self.max_workspaces {
            let entries: Vec<(Id, usize)> = self
                .buffers
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().len()))
                .collect();
            let max_key = entries
                .into_iter()
                .max_by_key(|(_, len)| *len)
                .map(|(key, _)| key);
            if let Some(key) = max_key {
                self.buffers.remove(&key);
            }
        }
    }

    /// Returns messages with created_at > since_ms, up to `limit` entries, oldest first.
    pub fn list_since(&self, workspace_id: &Id, since_ms: u64, limit: usize) -> Vec<Message> {
        match self.buffers.get(workspace_id) {
            None => vec![],
            Some(buf) => buf
                .iter()
                .filter(|m| m.created_at > since_ms)
                .take(limit)
                .cloned()
                .collect(),
        }
    }

    /// Returns messages across all workspaces with created_at > since_ms, up to `limit` entries.
    /// Used by the global activity endpoint for backward compatibility.
    pub fn list_all_since(&self, since_ms: u64, limit: usize) -> Vec<Message> {
        let mut all: Vec<Message> = self
            .buffers
            .iter()
            .flat_map(|entry| {
                entry
                    .value()
                    .iter()
                    .filter(|m| m.created_at > since_ms)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect();
        all.sort_by_key(|m| m.created_at);
        all.truncate(limit);
        all
    }
}

impl Default for TelemetryBuffer {
    fn default() -> Self {
        Self::new(10_000, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_telemetry(workspace_id: Option<Id>) -> Message {
        Message {
            id: Id::new("msg-1"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Server,
            workspace_id,
            to: Destination::Broadcast,
            kind: MessageKind::ToolCallStart,
            payload: None,
            created_at: 1_000_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    fn make_message(kind: MessageKind, workspace_id: Option<Id>) -> Message {
        Message {
            id: Id::new("msg-1"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Server,
            workspace_id,
            to: Destination::Broadcast,
            kind,
            payload: None,
            created_at: 1_000_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    // ── MessageKind serde ───────────────────────────────────────────────

    #[test]
    fn known_variants_serialize_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&MessageKind::TaskAssignment).unwrap(),
            "\"task_assignment\""
        );
        assert_eq!(
            serde_json::to_string(&MessageKind::AgentCreated).unwrap(),
            "\"agent_created\""
        );
        assert_eq!(
            serde_json::to_string(&MessageKind::ToolCallStart).unwrap(),
            "\"tool_call_start\""
        );
        assert_eq!(
            serde_json::to_string(&MessageKind::MrMerged).unwrap(),
            "\"mr_merged\""
        );
        assert_eq!(
            serde_json::to_string(&MessageKind::BudgetExhausted).unwrap(),
            "\"budget_exhausted\""
        );
    }

    #[test]
    fn known_variants_deserialize_from_snake_case() {
        let k: MessageKind = serde_json::from_str("\"task_assignment\"").unwrap();
        assert_eq!(k, MessageKind::TaskAssignment);

        let k: MessageKind = serde_json::from_str("\"agent_error\"").unwrap();
        assert_eq!(k, MessageKind::AgentError);

        let k: MessageKind = serde_json::from_str("\"run_finished\"").unwrap();
        assert_eq!(k, MessageKind::RunFinished);

        let k: MessageKind = serde_json::from_str("\"speculative_merge_clean\"").unwrap();
        assert_eq!(k, MessageKind::SpeculativeMergeClean);
    }

    #[test]
    fn custom_roundtrip() {
        let k = MessageKind::Custom("my_custom_event".to_string());
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, "\"my_custom_event\"");
        let back: MessageKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageKind::Custom("my_custom_event".to_string()));
    }

    #[test]
    fn unknown_string_becomes_custom() {
        let k: MessageKind = serde_json::from_str("\"some_future_event\"").unwrap();
        assert_eq!(k, MessageKind::Custom("some_future_event".to_string()));
    }

    #[test]
    fn all_known_variants_roundtrip() {
        let kinds = [
            MessageKind::TaskAssignment,
            MessageKind::ReviewRequest,
            MessageKind::StatusUpdate,
            MessageKind::Escalation,
            MessageKind::AgentCreated,
            MessageKind::AgentStatusChanged,
            MessageKind::AgentContainerSpawned,
            MessageKind::AgentCompleted,
            MessageKind::ReconciliationCompleted,
            MessageKind::TaskCreated,
            MessageKind::TaskTransitioned,
            MessageKind::MrCreated,
            MessageKind::MrStatusChanged,
            MessageKind::MrMerged,
            MessageKind::QueueUpdated,
            MessageKind::PushRejected,
            MessageKind::PushAccepted,
            MessageKind::SpecChanged,
            MessageKind::SpecApproved,
            MessageKind::GateFailure,
            MessageKind::StaleSpecWarning,
            MessageKind::SpeculativeConflict,
            MessageKind::SpeculativeMergeClean,
            MessageKind::HotFilesChanged,
            MessageKind::DataSeeded,
            MessageKind::BudgetWarning,
            MessageKind::BudgetExhausted,
            MessageKind::AgentError,
            MessageKind::ConstraintViolation,
            MessageKind::ToolCallStart,
            MessageKind::ToolCallEnd,
            MessageKind::TextMessageContent,
            MessageKind::RunStarted,
            MessageKind::RunFinished,
            MessageKind::StateChanged,
        ];
        for k in &kinds {
            let json = serde_json::to_string(k).unwrap();
            let back: MessageKind = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, k, "roundtrip failed for {:?}", k);
        }
    }

    // ── MessageKind methods ─────────────────────────────────────────────

    #[test]
    fn server_only_returns_true_for_event_builtins() {
        assert!(MessageKind::AgentCreated.server_only());
        assert!(MessageKind::GateFailure.server_only());
        assert!(MessageKind::DataSeeded.server_only());
        assert!(MessageKind::BudgetExhausted.server_only());
        assert!(MessageKind::AgentError.server_only());
        assert!(MessageKind::QueueUpdated.server_only());
    }

    #[test]
    fn server_only_returns_false_for_directed_telemetry_custom() {
        assert!(!MessageKind::TaskAssignment.server_only());
        assert!(!MessageKind::ToolCallStart.server_only());
        assert!(!MessageKind::Custom("foo".to_string()).server_only());
    }

    #[test]
    fn tier_directed() {
        assert_eq!(MessageKind::TaskAssignment.tier(), MessageTier::Directed);
        assert_eq!(MessageKind::Escalation.tier(), MessageTier::Directed);
    }

    #[test]
    fn tier_event() {
        assert_eq!(MessageKind::AgentCreated.tier(), MessageTier::Event);
        assert_eq!(MessageKind::MrMerged.tier(), MessageTier::Event);
        assert_eq!(
            MessageKind::Custom("x".to_string()).tier(),
            MessageTier::Event
        );
    }

    #[test]
    fn tier_telemetry() {
        assert_eq!(MessageKind::ToolCallStart.tier(), MessageTier::Telemetry);
        assert_eq!(MessageKind::StateChanged.tier(), MessageTier::Telemetry);
    }

    // ── Message struct serde ────────────────────────────────────────────

    #[test]
    fn message_roundtrip() {
        let msg = make_message(MessageKind::TaskAssignment, Some(Id::new("ws-1")));
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, msg.id);
        assert_eq!(back.kind, MessageKind::TaskAssignment);
        assert_eq!(back.workspace_id, Some(Id::new("ws-1")));
    }

    #[test]
    fn message_with_payload() {
        let mut msg = make_message(MessageKind::TaskAssignment, Some(Id::new("ws-1")));
        msg.payload = Some(json!({"task_id": "TASK-42", "spec_ref": null}));
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload.unwrap()["task_id"], "TASK-42");
    }

    #[test]
    fn message_origin_server_roundtrip() {
        let origin = MessageOrigin::Server;
        let json = serde_json::to_string(&origin).unwrap();
        let back: MessageOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageOrigin::Server);
    }

    #[test]
    fn message_origin_agent_roundtrip() {
        let origin = MessageOrigin::Agent(Id::new("agent-42"));
        let json = serde_json::to_string(&origin).unwrap();
        let back: MessageOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageOrigin::Agent(Id::new("agent-42")));
    }

    #[test]
    fn message_origin_user_roundtrip() {
        let origin = MessageOrigin::User(Id::new("user-99"));
        let json = serde_json::to_string(&origin).unwrap();
        let back: MessageOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageOrigin::User(Id::new("user-99")));
    }

    #[test]
    fn destination_broadcast_roundtrip() {
        let dest = Destination::Broadcast;
        let json = serde_json::to_string(&dest).unwrap();
        let back: Destination = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Destination::Broadcast);
    }

    #[test]
    fn destination_agent_roundtrip() {
        let dest = Destination::Agent(Id::new("agent-77"));
        let json = serde_json::to_string(&dest).unwrap();
        let back: Destination = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Destination::Agent(Id::new("agent-77")));
    }

    #[test]
    fn destination_workspace_roundtrip() {
        let dest = Destination::Workspace(Id::new("ws-99"));
        let json = serde_json::to_string(&dest).unwrap();
        let back: Destination = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Destination::Workspace(Id::new("ws-99")));
    }

    #[test]
    fn message_roundtrip_directed_from_agent_to_agent() {
        let msg = Message {
            id: Id::new("msg-dir-1"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Agent(Id::new("sender-agent")),
            workspace_id: Some(Id::new("ws-5")),
            to: Destination::Agent(Id::new("recv-agent")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 99_000,
            signature: Some("sig".to_string()),
            key_id: Some("kid-1".to_string()),
            acknowledged: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.from, MessageOrigin::Agent(Id::new("sender-agent")));
        assert_eq!(back.to, Destination::Agent(Id::new("recv-agent")));
        assert_eq!(back.signature, Some("sig".to_string()));
    }

    // ── TelemetryBuffer ─────────────────────────────────────────────────

    #[test]
    fn telemetry_buffer_push_and_list() {
        let buf = TelemetryBuffer::new(100, 1000);
        let ws = Id::new("ws-1");

        let mut msg = make_telemetry(Some(ws.clone()));
        msg.kind = MessageKind::ToolCallStart;
        msg.id = Id::new("m1");
        msg.created_at = 1000;
        buf.push(msg);

        let mut msg2 = make_telemetry(Some(ws.clone()));
        msg2.kind = MessageKind::ToolCallEnd;
        msg2.id = Id::new("m2");
        msg2.created_at = 2000;
        buf.push(msg2);

        let results = buf.list_since(&ws, 0, 100);
        assert_eq!(results.len(), 2);

        let results = buf.list_since(&ws, 1000, 100);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("m2"));
    }

    #[test]
    fn telemetry_buffer_per_workspace_eviction() {
        let buf = TelemetryBuffer::new(3, 1000);
        let ws = Id::new("ws-evict");

        for i in 0..5u64 {
            let mut msg = make_telemetry(Some(ws.clone()));
            msg.id = Id::new(format!("m{}", i));
            msg.created_at = i * 100;
            buf.push(msg);
        }

        let results = buf.list_since(&ws, 0, 100);
        // Only 3 newest entries remain
        assert_eq!(results.len(), 3);
        // The oldest two (m0, m1) were evicted
        assert_eq!(results[0].id, Id::new("m2"));
        assert_eq!(results[2].id, Id::new("m4"));
    }

    #[test]
    fn telemetry_buffer_workspace_isolation() {
        let buf = TelemetryBuffer::new(100, 1000);
        let ws_a = Id::new("ws-a");
        let ws_b = Id::new("ws-b");

        let mut msg = make_telemetry(Some(ws_a.clone()));
        msg.kind = MessageKind::RunStarted;
        msg.id = Id::new("msg-a");
        buf.push(msg);

        // ws-b has no messages
        let results = buf.list_since(&ws_b, 0, 100);
        assert_eq!(results.len(), 0);

        // ws-a has its message
        let results = buf.list_since(&ws_a, 0, 100);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn telemetry_buffer_rejects_non_telemetry_tier() {
        let buf = TelemetryBuffer::new(100, 1000);
        let ws = Id::new("ws-1");

        // Event-tier message — should be silently ignored
        let msg = make_message(MessageKind::AgentCreated, Some(ws.clone()));
        buf.push(msg);

        // Directed-tier message — should be silently ignored
        let msg = make_message(MessageKind::TaskAssignment, Some(ws.clone()));
        buf.push(msg);

        assert_eq!(buf.list_since(&ws, 0, 100).len(), 0);
    }

    #[test]
    fn telemetry_buffer_rejects_no_workspace() {
        let buf = TelemetryBuffer::new(100, 1000);
        let msg = make_telemetry(None); // workspace_id: None
        buf.push(msg);

        let ws = Id::new("any");
        assert_eq!(buf.list_since(&ws, 0, 100).len(), 0);
    }

    #[test]
    fn telemetry_buffer_workspace_count_eviction() {
        // max 2 workspaces
        let buf = TelemetryBuffer::new(1000, 2);
        let ws_a = Id::new("ws-a");
        let ws_b = Id::new("ws-b");
        let ws_c = Id::new("ws-c");

        // Fill ws_a with 5 messages (most entries); created_at starts at 1 so since_ms=0 filter includes all
        for i in 0..5u64 {
            let mut msg = make_telemetry(Some(ws_a.clone()));
            msg.id = Id::new(format!("a{}", i));
            msg.created_at = i + 1;
            buf.push(msg);
        }
        // Fill ws_b with 2 messages
        for i in 0..2u64 {
            let mut msg = make_telemetry(Some(ws_b.clone()));
            msg.id = Id::new(format!("b{}", i));
            msg.created_at = i + 1;
            buf.push(msg);
        }
        // Adding ws_c pushes us to 3 workspaces — should evict the largest (ws_a, 5 entries)
        let mut msg = make_telemetry(Some(ws_c.clone()));
        msg.id = Id::new("c0");
        msg.created_at = 1;
        buf.push(msg);

        // ws_a should be evicted (it was the largest)
        assert_eq!(buf.list_since(&ws_a, 0, 100).len(), 0);
        // ws_b and ws_c should still be present
        assert_eq!(buf.list_since(&ws_b, 0, 100).len(), 2);
        assert_eq!(buf.list_since(&ws_c, 0, 100).len(), 1);
    }

    #[test]
    fn telemetry_buffer_list_since_limit_truncates() {
        let buf = TelemetryBuffer::new(1000, 100);
        let ws = Id::new("ws-limit");
        for i in 1u64..=10 {
            let mut msg = make_telemetry(Some(ws.clone()));
            msg.id = Id::new(format!("m{}", i));
            msg.created_at = i;
            buf.push(msg);
        }
        // All 10 messages have created_at > 0; limit to 3
        let results = buf.list_since(&ws, 0, 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn telemetry_buffer_default_has_expected_capacity() {
        let buf = TelemetryBuffer::default();
        let ws = Id::new("ws-default");
        // Default should accept pushes without panicking
        let msg = make_telemetry(Some(ws.clone()));
        buf.push(msg);
        assert_eq!(buf.list_since(&ws, 0, 100).len(), 1);
    }
}
