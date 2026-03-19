//! Agent message inbox types.

use gyre_domain::MessageType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub from: String,
    pub content: serde_json::Value,
    pub message_type: Option<MessageType>,
    pub created_at: u64,
}
