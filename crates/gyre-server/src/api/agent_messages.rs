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

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_agent(app: Router, name: &str) -> (Router, String) {
        let body = serde_json::json!({ "name": name });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn send_and_receive_message() {
        let app = app();
        let (app, id) = create_agent(app, "msg-agent").await;

        let body = serde_json::json!({ "from": "CEO", "content": "hello agent" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["from"], "CEO");
        assert_eq!(json["content"], "hello agent");

        // Poll -- should drain
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["from"], "CEO");

        // Second poll -- inbox empty
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn send_typed_task_assignment_message() {
        let app = app();
        let (app, id) = create_agent(app, "typed-agent").await;

        let body = serde_json::json!({
            "from": "CEO",
            "content": "assign task",
            "message_type": {
                "type": "task_assignment",
                "task_id": "task-42",
                "spec_ref": "specs/foo.md"
            }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["message_type"]["type"], "task_assignment");
        assert_eq!(json["message_type"]["task_id"], "task-42");

        // Poll and verify typed message
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let msgs = body_json(resp).await;
        let msgs = msgs.as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["message_type"]["type"], "task_assignment");
    }

    #[tokio::test]
    async fn send_typed_review_request_message() {
        let app = app();
        let (app, id) = create_agent(app, "review-agent").await;

        let body = serde_json::json!({
            "from": "CEO",
            "content": "please review",
            "message_type": {
                "type": "review_request",
                "mr_id": "mr-7"
            }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["message_type"]["type"], "review_request");
        assert_eq!(json["message_type"]["mr_id"], "mr-7");
    }

    #[tokio::test]
    async fn send_message_to_unknown_agent() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/ghost/messages")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"from":"x","content":"y"})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn multiple_messages_queued() {
        let app = app();
        let (app, id) = create_agent(app, "queue-agent").await;

        for i in 0..3u32 {
            let body = serde_json::json!({ "from": "CEO", "content": i });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/agents/{id}/messages"))
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/agents/{id}/messages"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 3);
    }
}
