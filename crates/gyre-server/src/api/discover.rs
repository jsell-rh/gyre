use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AgentCard, AgentStatus};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

#[derive(Deserialize)]
pub struct DiscoverQuery {
    pub capability: Option<String>,
}

/// GET /api/v1/agents/discover -- return Agent Cards for all active agents.
/// Optional ?capability=<cap> filter.
pub async fn discover_agents(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DiscoverQuery>,
) -> Result<Json<Vec<AgentCard>>, ApiError> {
    // Get all active agents
    let agents = state.agents.list_by_status(&AgentStatus::Active).await?;

    let mut result: Vec<AgentCard> = Vec::new();
    for a in &agents {
        if let Ok(Some(json)) = state.kv_store.kv_get("agent_cards", a.id.as_str()).await {
            if let Ok(card) = serde_json::from_str::<AgentCard>(&json) {
                result.push(card);
            }
        }
    }

    // Filter by capability if requested
    if let Some(cap) = &params.capability {
        result.retain(|card| card.capabilities.iter().any(|c| c == cap));
    }

    Ok(Json(result))
}

/// GET /api/v1/agents/{id}/card -- retrieve an agent's Agent Card.
pub async fn get_agent_card(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Option<AgentCard>>, ApiError> {
    state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    let card = if let Ok(Some(json)) = state.kv_store.kv_get("agent_cards", &id).await {
        serde_json::from_str::<AgentCard>(&json).ok()
    } else {
        None
    };

    Ok(Json(card))
}

/// PUT /api/v1/agents/{id}/card -- update an agent's own Agent Card.
pub async fn update_agent_card(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(card): Json<AgentCard>,
) -> Result<(StatusCode, Json<AgentCard>), ApiError> {
    state
        .agents
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    if card.agent_id.as_str() != id {
        return Err(ApiError::InvalidInput(
            "card agent_id must match path id".to_string(),
        ));
    }

    let json = serde_json::to_string(&card).map_err(|e| ApiError::Internal(e.into()))?;
    state
        .kv_store
        .kv_set("agent_cards", &id, json)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::OK, Json(card)))
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

    async fn create_active_agent(app: Router, name: &str) -> (Router, String) {
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

        // Activate the agent
        let status_body = serde_json::json!({ "status": "active" });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&status_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        (app, id)
    }

    #[tokio::test]
    async fn discover_returns_empty_when_no_cards() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/discover")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn discover_returns_active_agent_cards() {
        let app = app();
        let (app, id) = create_active_agent(app, "discover-agent").await;

        // Register a card
        let card = serde_json::json!({
            "agent_id": id,
            "name": "discover-agent",
            "description": "A test agent",
            "capabilities": ["rust-dev", "testing"],
            "protocols": ["a2a"],
            "endpoint": null
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/card"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&card).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Discover
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/discover")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let cards = json.as_array().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["name"], "discover-agent");
    }

    #[tokio::test]
    async fn discover_filter_by_capability() {
        let app = app();
        let (app, id1) = create_active_agent(app, "rust-agent").await;
        let (app, id2) = create_active_agent(app, "review-agent").await;

        let card1 = serde_json::json!({
            "agent_id": id1,
            "name": "rust-agent",
            "description": "Rust dev",
            "capabilities": ["rust-dev"],
            "protocols": ["a2a"],
            "endpoint": null
        });
        let card2 = serde_json::json!({
            "agent_id": id2,
            "name": "review-agent",
            "description": "Reviewer",
            "capabilities": ["review"],
            "protocols": ["a2a"],
            "endpoint": null
        });

        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id1}/card"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&card1).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id2}/card"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&card2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Filter by rust-dev
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/discover?capability=rust-dev")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let cards = json.as_array().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["name"], "rust-agent");
    }

    #[tokio::test]
    async fn update_card_rejects_mismatched_id() {
        let app = app();
        let (app, id) = create_active_agent(app, "mismatch-agent").await;

        let card = serde_json::json!({
            "agent_id": "wrong-id",
            "name": "mismatch-agent",
            "description": "",
            "capabilities": [],
            "protocols": [],
            "endpoint": null
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/agents/{id}/card"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&card).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_card_for_unknown_agent() {
        let card = serde_json::json!({
            "agent_id": "ghost",
            "name": "ghost",
            "description": "",
            "capabilities": [],
            "protocols": [],
            "endpoint": null
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/agents/ghost/card")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&card).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
