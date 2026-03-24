use crate::Id;
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::VecDeque;

/// The unified message envelope — all inter-component communication flows through this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// Wire: `{"server": null}`, `{"agent": "<id>"}`, `{"user": "<id>"}`.
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    TaskCreated,
    TaskTransitioned,
    MrCreated,
    MrStatusChanged,
    MrMerged,
    QueueUpdated,
    PushRejected,
    PushAccepted,
    SpecChanged,
    GateFailure,
    StaleSpecWarning,
    SpeculativeConflict,
    SpeculativeMergeClean,
    HotFilesChanged,
    DataSeeded,
    BudgetWarning,
    BudgetExhausted,
    AgentError,

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
            MessageKind::TaskCreated => "task_created",
            MessageKind::TaskTransitioned => "task_transitioned",
            MessageKind::MrCreated => "mr_created",
            MessageKind::MrStatusChanged => "mr_status_changed",
            MessageKind::MrMerged => "mr_merged",
            MessageKind::QueueUpdated => "queue_updated",
            MessageKind::PushRejected => "push_rejected",
            MessageKind::PushAccepted => "push_accepted",
            MessageKind::SpecChanged => "spec_changed",
            MessageKind::GateFailure => "gate_failure",
            MessageKind::StaleSpecWarning => "stale_spec_warning",
            MessageKind::SpeculativeConflict => "speculative_conflict",
            MessageKind::SpeculativeMergeClean => "speculative_merge_clean",
            MessageKind::HotFilesChanged => "hot_files_changed",
            MessageKind::DataSeeded => "data_seeded",
            MessageKind::BudgetWarning => "budget_warning",
            MessageKind::BudgetExhausted => "budget_exhausted",
            MessageKind::AgentError => "agent_error",
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
                | MessageKind::TaskCreated
                | MessageKind::TaskTransitioned
                | MessageKind::MrCreated
                | MessageKind::MrStatusChanged
                | MessageKind::MrMerged
                | MessageKind::QueueUpdated
                | MessageKind::PushRejected
                | MessageKind::PushAccepted
                | MessageKind::SpecChanged
                | MessageKind::GateFailure
                | MessageKind::StaleSpecWarning
                | MessageKind::SpeculativeConflict
                | MessageKind::SpeculativeMergeClean
                | MessageKind::HotFilesChanged
                | MessageKind::DataSeeded
                | MessageKind::BudgetWarning
                | MessageKind::BudgetExhausted
                | MessageKind::AgentError
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
            | MessageKind::TaskCreated
            | MessageKind::TaskTransitioned
            | MessageKind::MrCreated
            | MessageKind::MrStatusChanged
            | MessageKind::MrMerged
            | MessageKind::QueueUpdated
            | MessageKind::PushRejected
            | MessageKind::PushAccepted
            | MessageKind::SpecChanged
            | MessageKind::GateFailure
            | MessageKind::StaleSpecWarning
            | MessageKind::SpeculativeConflict
            | MessageKind::SpeculativeMergeClean
            | MessageKind::HotFilesChanged
            | MessageKind::DataSeeded
            | MessageKind::BudgetWarning
            | MessageKind::BudgetExhausted
            | MessageKind::AgentError
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
            "task_created" => MessageKind::TaskCreated,
            "task_transitioned" => MessageKind::TaskTransitioned,
            "mr_created" => MessageKind::MrCreated,
            "mr_status_changed" => MessageKind::MrStatusChanged,
            "mr_merged" => MessageKind::MrMerged,
            "queue_updated" => MessageKind::QueueUpdated,
            "push_rejected" => MessageKind::PushRejected,
            "push_accepted" => MessageKind::PushAccepted,
            "spec_changed" => MessageKind::SpecChanged,
            "gate_failure" => MessageKind::GateFailure,
            "stale_spec_warning" => MessageKind::StaleSpecWarning,
            "speculative_conflict" => MessageKind::SpeculativeConflict,
            "speculative_merge_clean" => MessageKind::SpeculativeMergeClean,
            "hot_files_changed" => MessageKind::HotFilesChanged,
            "data_seeded" => MessageKind::DataSeeded,
            "budget_warning" => MessageKind::BudgetWarning,
            "budget_exhausted" => MessageKind::BudgetExhausted,
            "agent_error" => MessageKind::AgentError,
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
pub struct TelemetryBuffer {
    /// Per-workspace ring buffers. Key: workspace_id.
    buffers: DashMap<Id, VecDeque<Message>>,
    /// Maximum entries per workspace before eviction (default 10,000).
    max_per_workspace: usize,
}

impl TelemetryBuffer {
    pub fn new(max_per_workspace: usize) -> Self {
        Self {
            buffers: DashMap::new(),
            max_per_workspace,
        }
    }

    /// Push a telemetry message into the workspace buffer.
    /// Evicts the oldest entry when max_per_workspace is exceeded.
    pub fn push(&self, message: Message) {
        let workspace_id = match &message.workspace_id {
            Some(id) => id.clone(),
            None => return, // Telemetry requires a workspace scope
        };
        let mut buf = self.buffers.entry(workspace_id).or_default();
        buf.push_back(message);
        if buf.len() > self.max_per_workspace {
            buf.pop_front();
        }
    }

    /// Returns messages with created_at > since_ms, up to limit entries, oldest first.
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
}

impl Default for TelemetryBuffer {
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            MessageKind::TaskCreated,
            MessageKind::TaskTransitioned,
            MessageKind::MrCreated,
            MessageKind::MrStatusChanged,
            MessageKind::MrMerged,
            MessageKind::QueueUpdated,
            MessageKind::PushRejected,
            MessageKind::PushAccepted,
            MessageKind::SpecChanged,
            MessageKind::GateFailure,
            MessageKind::StaleSpecWarning,
            MessageKind::SpeculativeConflict,
            MessageKind::SpeculativeMergeClean,
            MessageKind::HotFilesChanged,
            MessageKind::DataSeeded,
            MessageKind::BudgetWarning,
            MessageKind::BudgetExhausted,
            MessageKind::AgentError,
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
    fn destination_broadcast_roundtrip() {
        let dest = Destination::Broadcast;
        let json = serde_json::to_string(&dest).unwrap();
        let back: Destination = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Destination::Broadcast);
    }

    #[test]
    fn destination_workspace_roundtrip() {
        let dest = Destination::Workspace(Id::new("ws-99"));
        let json = serde_json::to_string(&dest).unwrap();
        let back: Destination = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Destination::Workspace(Id::new("ws-99")));
    }

    // ── TelemetryBuffer ─────────────────────────────────────────────────

    #[test]
    fn telemetry_buffer_push_and_list() {
        let buf = TelemetryBuffer::new(100);
        let ws = Id::new("ws-1");

        let mut msg = make_message(MessageKind::ToolCallStart, Some(ws.clone()));
        msg.id = Id::new("m1");
        msg.created_at = 1000;
        buf.push(msg);

        let mut msg2 = make_message(MessageKind::ToolCallEnd, Some(ws.clone()));
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
    fn telemetry_buffer_eviction() {
        let buf = TelemetryBuffer::new(3);
        let ws = Id::new("ws-evict");

        for i in 0..5u64 {
            let mut msg = make_message(MessageKind::ToolCallStart, Some(ws.clone()));
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
        let buf = TelemetryBuffer::new(100);
        let ws_a = Id::new("ws-a");
        let ws_b = Id::new("ws-b");

        let mut msg = make_message(MessageKind::RunStarted, Some(ws_a.clone()));
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
    fn telemetry_buffer_ignores_broadcast_messages() {
        let buf = TelemetryBuffer::new(100);
        let msg = make_message(MessageKind::DataSeeded, None); // workspace_id: None
        buf.push(msg);

        // Nothing was stored (no workspace key)
        let ws = Id::new("any");
        assert_eq!(buf.list_since(&ws, 0, 100).len(), 0);
    }
}
