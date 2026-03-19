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
