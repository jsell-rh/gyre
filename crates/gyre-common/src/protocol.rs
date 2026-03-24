use crate::Id;
use serde::{Deserialize, Serialize};

/// AG-UI typed event taxonomy (replaces free-form event_type strings).
///
/// These types align with the AG-UI protocol for agent activity events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgEventType {
    ToolCallStart,
    ToolCallEnd,
    TextMessageContent,
    RunStarted,
    RunFinished,
    StateChanged,
    Error,
    /// Catch-all for unknown or legacy event type strings.
    Custom(String),
}

impl AgEventType {
    pub fn as_str(&self) -> &str {
        match self {
            AgEventType::ToolCallStart => "TOOL_CALL_START",
            AgEventType::ToolCallEnd => "TOOL_CALL_END",
            AgEventType::TextMessageContent => "TEXT_MESSAGE_CONTENT",
            AgEventType::RunStarted => "RUN_STARTED",
            AgEventType::RunFinished => "RUN_FINISHED",
            AgEventType::StateChanged => "STATE_CHANGED",
            AgEventType::Error => "ERROR",
            AgEventType::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for AgEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for AgEventType {
    fn from(s: &str) -> Self {
        match s {
            "TOOL_CALL_START" => AgEventType::ToolCallStart,
            "TOOL_CALL_END" => AgEventType::ToolCallEnd,
            "TEXT_MESSAGE_CONTENT" => AgEventType::TextMessageContent,
            "RUN_STARTED" => AgEventType::RunStarted,
            "RUN_FINISHED" => AgEventType::RunFinished,
            "STATE_CHANGED" => AgEventType::StateChanged,
            "ERROR" => AgEventType::Error,
            other => AgEventType::Custom(other.to_string()),
        }
    }
}

impl From<String> for AgEventType {
    fn from(s: String) -> Self {
        AgEventType::from(s.as_str())
    }
}

impl Serialize for AgEventType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgEventType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(AgEventType::from(s.as_str()))
    }
}

/// Activity event data shared between server and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEventData {
    pub event_id: String,
    pub agent_id: String,
    pub event_type: AgEventType,
    pub description: String,
    pub timestamp: u64,
}

/// Workspace scope for a WebSocket subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeScope {
    pub workspace_id: Id,
}

/// WebSocket message types shared between server and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    Ping {
        timestamp: u64,
    },
    Pong {
        timestamp: u64,
    },
    Auth {
        token: String,
    },
    AuthResult {
        success: bool,
        message: String,
    },
    ActivityEvent {
        event_id: String,
        agent_id: String,
        event_type: AgEventType,
        description: String,
        timestamp: u64,
    },
    ActivityQuery {
        since: Option<u64>,
        limit: Option<usize>,
    },
    ActivityResponse {
        events: Vec<ActivityEventData>,
    },
    /// Subscribe to workspace-scoped message delivery.
    /// `last_seen` is epoch milliseconds — when present the server replays
    /// persisted Event-tier messages with created_at > last_seen.
    Subscribe {
        scopes: Vec<SubscribeScope>,
        last_seen: Option<u64>,
    },
    /// Sent by the server when replay was truncated at 1000 messages.
    /// The client can fetch the full history via GET /api/v1/workspaces/:id/messages.
    ReplayCatchUp {
        truncated: bool,
    },
    /// Catch-all for unrecognized message types — silently ignored for forward compatibility.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_roundtrip() {
        let msg = WsMessage::Ping { timestamp: 999 };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsMessage::Ping { timestamp: 999 }));
    }

    #[test]
    fn pong_roundtrip() {
        let msg = WsMessage::Pong { timestamp: 42 };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsMessage::Pong { timestamp: 42 }));
    }

    #[test]
    fn auth_roundtrip() {
        let msg = WsMessage::Auth {
            token: "my-token".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsMessage::Auth { .. }));
        if let WsMessage::Auth { token } = decoded {
            assert_eq!(token, "my-token");
        }
    }

    #[test]
    fn auth_result_roundtrip() {
        let msg = WsMessage::AuthResult {
            success: true,
            message: "OK".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            decoded,
            WsMessage::AuthResult { success: true, .. }
        ));
    }

    #[test]
    fn ping_json_has_type_tag() {
        let msg = WsMessage::Ping { timestamp: 1 };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"Ping\""));
    }

    #[test]
    fn ag_event_type_serialization() {
        assert_eq!(
            serde_json::to_string(&AgEventType::ToolCallStart).unwrap(),
            "\"TOOL_CALL_START\""
        );
        assert_eq!(
            serde_json::to_string(&AgEventType::RunStarted).unwrap(),
            "\"RUN_STARTED\""
        );
        assert_eq!(
            serde_json::to_string(&AgEventType::StateChanged).unwrap(),
            "\"STATE_CHANGED\""
        );
    }

    #[test]
    fn ag_event_type_deserialization() {
        let et: AgEventType = serde_json::from_str("\"TOOL_CALL_START\"").unwrap();
        assert_eq!(et, AgEventType::ToolCallStart);

        let et: AgEventType = serde_json::from_str("\"UNKNOWN_LEGACY\"").unwrap();
        assert_eq!(et, AgEventType::Custom("UNKNOWN_LEGACY".to_string()));
    }

    #[test]
    fn activity_event_data_roundtrip() {
        let ev = ActivityEventData {
            event_id: "ev1".to_string(),
            agent_id: "agent-a".to_string(),
            event_type: AgEventType::RunStarted,
            description: "Agent started a run".to_string(),
            timestamp: 1000,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let decoded: ActivityEventData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event_type, AgEventType::RunStarted);
        assert_eq!(decoded.event_type.as_str(), "RUN_STARTED");
    }

    #[test]
    fn ws_subscribe_roundtrip() {
        use crate::Id;
        let msg = WsMessage::Subscribe {
            scopes: vec![SubscribeScope { workspace_id: Id::new("ws-42") }],
            last_seen: Some(1_711_324_800_000),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"Subscribe\""));
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        if let WsMessage::Subscribe { scopes, last_seen } = decoded {
            assert_eq!(scopes.len(), 1);
            assert_eq!(scopes[0].workspace_id, Id::new("ws-42"));
            assert_eq!(last_seen, Some(1_711_324_800_000));
        } else {
            panic!("expected Subscribe variant");
        }
    }

    #[test]
    fn ws_replay_catch_up_roundtrip() {
        let msg = WsMessage::ReplayCatchUp { truncated: true };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"ReplayCatchUp\""));
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, WsMessage::ReplayCatchUp { truncated: true }));
    }

    #[test]
    fn ws_unknown_variant_ignored() {
        // An unrecognized type tag should deserialize to Unknown without error.
        let json = r#"{"type":"FutureUnknownVariant","extra_field":42}"#;
        let decoded: WsMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(decoded, WsMessage::Unknown));
    }

    #[test]
    fn ws_subscribe_null_last_seen() {
        use crate::Id;
        let msg = WsMessage::Subscribe {
            scopes: vec![SubscribeScope { workspace_id: Id::new("ws-1") }],
            last_seen: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: WsMessage = serde_json::from_str(&json).unwrap();
        if let WsMessage::Subscribe { last_seen, .. } = decoded {
            assert_eq!(last_seen, None);
        } else {
            panic!("expected Subscribe");
        }
    }
}
