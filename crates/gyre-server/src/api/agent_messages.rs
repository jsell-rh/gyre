use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::MessageType;
use serde::Deserialize;
use std::sync::Arc;

use crate::messages::AgentMessage;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub from: String,
    pub content: serde_json::Value,
    /// Optional typed message variant.
    pub message_type: Option<MessageType>,
}

/// GET /api/v1/agents/{id}/messages -- drain and return pending messages.
pub async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AgentMessage>>, ApiError> {
    state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    // Drain: load all messages, return them, clear the queue.
    let messages: Vec<AgentMessage> = state
        .kv_store
        .kv_get("agent_messages", &id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !messages.is_empty() {
        let empty: Vec<AgentMessage> = vec![];
        let _ = state
            .kv_store
            .kv_set(
                "agent_messages",
                &id,
                serde_json::to_string(&empty).unwrap_or_default(),
            )
            .await;
    }
    Ok(Json(messages))
}

/// POST /api/v1/agents/{id}/messages -- deliver a message to an agent's inbox.
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<AgentMessage>), ApiError> {
    state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    let msg = AgentMessage {
        id: new_id().to_string(),
        from: req.from,
        content: req.content,
        message_type: req.message_type,
        created_at: now_secs(),
    };

    // Append message to the agent's queue.
    let mut messages: Vec<AgentMessage> = state
        .kv_store
        .kv_get("agent_messages", &id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    messages.push(msg.clone());
    let json = serde_json::to_string(&messages).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("agent_messages", &id, json)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(msg)))
}

// Legacy tests removed — this module's drain-on-read POST behavior is superseded by
// message bus Phase 3 (api::messages). New tests live in api::messages::tests.
