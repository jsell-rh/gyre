use serde::{Deserialize, Serialize};

/// WebSocket message types shared between server and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
    Auth { token: String },
    AuthResult { success: bool, message: String },
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
}
