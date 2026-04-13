//! Shared message signing logic.
//!
//! Provides a single implementation of the Ed25519 signing algorithm for the
//! unified message bus (message-bus.md §Signing). Used by both REST handlers
//! (`api/messages.rs`) and MCP handlers (`mcp.rs`).

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gyre_common::message::Message;
use sha2::{Digest, Sha256};

use crate::AppState;

/// Sign a message with the server's Ed25519 key.
/// Returns `(signature_b64, key_id)`.
pub fn sign_message(state: &AppState, msg: &Message) -> (String, String) {
    let (from_type, from_id) = match &msg.from {
        gyre_common::message::MessageOrigin::Server => ("server", "".to_string()),
        gyre_common::message::MessageOrigin::Agent(id) => ("agent", id.as_str().to_string()),
        gyre_common::message::MessageOrigin::User(id) => ("user", id.as_str().to_string()),
    };
    let (to_type, to_id) = match &msg.to {
        gyre_common::message::Destination::Agent(id) => ("agent", id.as_str().to_string()),
        gyre_common::message::Destination::Workspace(id) => ("workspace", id.as_str().to_string()),
        gyre_common::message::Destination::Broadcast => ("broadcast", "".to_string()),
    };
    let ws_id = msg
        .workspace_id
        .as_ref()
        .map(|id| id.as_str().to_string())
        .unwrap_or_default();

    let payload_json = msg
        .payload
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();

    let mut hasher = Sha256::new();
    hasher.update(payload_json.as_bytes());
    let payload_hash = format!("{:x}", hasher.finalize());

    let sign_input = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        msg.id.as_str(),
        from_type,
        from_id,
        ws_id,
        to_type,
        to_id,
        msg.kind.as_str(),
        payload_hash,
        msg.created_at,
    );

    let sig_bytes = state.agent_signing_key.sign_bytes(sign_input.as_bytes());
    let sig_b64 = B64.encode(&sig_bytes);
    (sig_b64, state.agent_signing_key.kid.clone())
}
