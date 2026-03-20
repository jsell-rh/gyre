use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::NetworkPeer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct RegisterPeerRequest {
    pub agent_id: String,
    pub wireguard_pubkey: String,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

#[derive(Serialize)]
pub struct NetworkPeerResponse {
    pub id: String,
    pub agent_id: String,
    pub wireguard_pubkey: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub registered_at: u64,
    pub last_seen: Option<u64>,
}

impl From<NetworkPeer> for NetworkPeerResponse {
    fn from(p: NetworkPeer) -> Self {
        Self {
            id: p.id.to_string(),
            agent_id: p.agent_id.to_string(),
            wireguard_pubkey: p.wireguard_pubkey,
            endpoint: p.endpoint,
            allowed_ips: p.allowed_ips,
            registered_at: p.registered_at,
            last_seen: p.last_seen,
        }
    }
}

/// POST /api/v1/network/peers — register a WireGuard peer.
pub async fn register_peer(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterPeerRequest>,
) -> Result<(StatusCode, Json<NetworkPeerResponse>), ApiError> {
    let peer = NetworkPeer::new(
        new_id(),
        Id::new(req.agent_id),
        req.wireguard_pubkey,
        req.endpoint,
        req.allowed_ips,
        now_secs(),
    );
    state.network_peers.register(&peer).await?;
    Ok((StatusCode::CREATED, Json(NetworkPeerResponse::from(peer))))
}

/// GET /api/v1/network/peers — list all peers (mesh topology).
pub async fn list_peers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NetworkPeerResponse>>, ApiError> {
    let peers = state.network_peers.list().await?;
    Ok(Json(
        peers.into_iter().map(NetworkPeerResponse::from).collect(),
    ))
}

/// GET /api/v1/network/peers/{agent_id} — get peer for a specific agent.
pub async fn get_peer_by_agent(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
) -> Result<Json<NetworkPeerResponse>, ApiError> {
    let peer = state
        .network_peers
        .find_by_agent(&Id::new(&agent_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no peer for agent {agent_id}")))?;
    Ok(Json(NetworkPeerResponse::from(peer)))
}

/// DELETE /api/v1/network/peers/{id} — deregister a peer.
pub async fn delete_peer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.network_peers.delete(&Id::new(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/network/derp-map — return DERP relay map (stub).
pub async fn derp_map() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "regions": {
            "1": {
                "regionID": 1,
                "regionCode": "gyre-default",
                "regionName": "Gyre Default DERP",
                "nodes": []
            }
        }
    }))
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

    #[tokio::test]
    async fn register_peer_created() {
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "wireguard_pubkey": "abc123==",
            "endpoint": "10.0.0.1:51820",
            "allowed_ips": ["10.100.0.1/32"]
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["agent_id"], "agent-1");
        assert_eq!(json["wireguard_pubkey"], "abc123==");
        assert!(json["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn list_peers_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn register_and_list_peers() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-2",
            "wireguard_pubkey": "key2==",
            "allowed_ips": []
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_peer_by_agent_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers/agent/ghost-agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn register_then_get_by_agent() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-lookup",
            "wireguard_pubkey": "lookupkey==",
            "allowed_ips": ["10.0.0.1/32"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers/agent/agent-lookup")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["wireguard_pubkey"], "lookupkey==");
    }

    #[tokio::test]
    async fn delete_peer_no_content() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-del",
            "wireguard_pubkey": "delkey==",
            "allowed_ips": []
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let peer_id = json["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/network/peers/{peer_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn derp_map_returns_regions() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/derp-map")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["regions"].is_object());
    }
}
