use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::NetworkPeer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;
use gyre_domain::UserRole;

use super::error::ApiError;
use super::{new_id, now_secs};

/// Validate a WireGuard public key: must be 44-char (or 43-char no-padding) base64 of 32 bytes.
fn validate_wg_pubkey(key: &str) -> bool {
    let padded;
    let to_decode = match key.len() {
        44 => key,
        43 => {
            padded = format!("{key}=");
            &padded
        }
        _ => return false,
    };
    // Manual base64 decode: count valid chars and decoded length.
    // Accept both standard (+/) and URL-safe (-_) variants.
    let mut byte_count = 0usize;
    let mut pad_count = 0usize;
    for b in to_decode.bytes() {
        if b == b'=' {
            pad_count += 1;
        } else if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'-' || b == b'_' {
            if pad_count > 0 {
                return false; // padding before end
            }
            byte_count += 1;
        } else {
            return false; // invalid char
        }
    }
    // total length must be multiple of 4 (with padding counted)
    if !(byte_count + pad_count).is_multiple_of(4) {
        return false;
    }
    // Decoded bytes = 3 * groups - padding_chars
    let groups = (byte_count + pad_count) / 4;
    let decoded_len = groups * 3 - pad_count;
    decoded_len == 32
}

#[derive(Deserialize)]
pub struct RegisterPeerRequest {
    pub agent_id: String,
    pub wireguard_pubkey: String,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdatePeerEndpointRequest {
    pub endpoint: String,
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
    pub mesh_ip: Option<String>,
    pub is_stale: bool,
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
            mesh_ip: p.mesh_ip,
            is_stale: p.is_stale,
        }
    }
}

/// POST /api/v1/network/peers — register a WireGuard peer.
///
/// Security (M26.4):
/// - JWT callers must match the `agent_id` being registered (ownership enforcement).
/// - Non-JWT callers (global token, API key) require Admin role.
/// - Pubkey must be a valid 32-byte Curve25519 key in base64 (M26.4).
/// - Mesh IP allocated from GYRE_WG_CIDR pool when WireGuard is enabled (M26.1).
pub async fn register_peer(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<RegisterPeerRequest>,
) -> Result<(StatusCode, Json<NetworkPeerResponse>), ApiError> {
    // Validate pubkey format.
    if !validate_wg_pubkey(&req.wireguard_pubkey) {
        return Err(ApiError::InvalidInput(
            "invalid WireGuard public key: must be 44-char base64 encoding of 32 bytes".to_string(),
        ));
    }

    // Ownership enforcement (M26.4):
    // - JWT bearer (agent_id != "system"): must register its own key only.
    // - Global token / API key (jwt_claims is None): must be Admin.
    if auth.jwt_claims.is_some() {
        // JWT caller: sub must match agent_id.
        if auth.agent_id != req.agent_id {
            return Err(ApiError::Forbidden(
                "JWT callers may only register their own WireGuard pubkey".to_string(),
            ));
        }
    } else {
        // Non-JWT (global token, API key): require Admin.
        if !auth.roles.contains(&UserRole::Admin) {
            return Err(ApiError::Forbidden(
                "Admin role required to register a peer for another agent".to_string(),
            ));
        }
    }

    // Allocate mesh IP if WireGuard is enabled.
    let mesh_ip = state.wg_config.allocate_ip();

    // Use the allocated mesh IP as allowed_ips if none provided.
    let allowed_ips = if req.allowed_ips.is_empty() {
        mesh_ip
            .as_ref()
            .map(|ip| vec![format!("{ip}/32")])
            .unwrap_or_default()
    } else {
        req.allowed_ips
    };

    let mut peer = NetworkPeer::new(
        new_id(),
        Id::new(req.agent_id),
        req.wireguard_pubkey,
        req.endpoint,
        allowed_ips,
        now_secs(),
    );
    peer.mesh_ip = mesh_ip;

    state.network_peers.register(&peer).await?;
    Ok((StatusCode::CREATED, Json(NetworkPeerResponse::from(peer))))
}

/// GET /api/v1/network/peers — list all non-stale peers (mesh topology).
pub async fn list_peers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NetworkPeerResponse>>, ApiError> {
    let now = now_secs();
    let ttl = state.wg_config.peer_ttl_secs;
    let peers = state.network_peers.list().await?;
    // Filter out stale peers (older than TTL) for the distributed peer list.
    let fresh: Vec<NetworkPeerResponse> = peers
        .into_iter()
        .filter(|p| {
            if p.is_stale {
                return false;
            }
            // Also filter by freshness: last_seen or registered_at must be within TTL.
            let age = match p.last_seen {
                Some(ts) => now.saturating_sub(ts),
                None => now.saturating_sub(p.registered_at),
            };
            age <= ttl
        })
        .map(NetworkPeerResponse::from)
        .collect();
    Ok(Json(fresh))
}

/// GET /api/v1/network/peers/agent/{agent_id} — get peer for a specific agent.
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

/// PUT /api/v1/network/peers/{id} — update endpoint (roaming support, M26.1).
///
/// JWT callers may only update their own peer record. Admin can update any.
pub async fn update_peer_endpoint(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdatePeerEndpointRequest>,
) -> Result<StatusCode, ApiError> {
    // Fetch the peer to check ownership.
    // We need to find by id — list and filter is fine for now.
    let peers = state.network_peers.list().await?;
    let peer = peers
        .into_iter()
        .find(|p| p.id.as_str() == id)
        .ok_or_else(|| ApiError::NotFound(format!("peer {id} not found")))?;

    // Ownership check: JWT caller must own the peer record.
    if auth.jwt_claims.is_some() && auth.agent_id != peer.agent_id.as_str() {
        return Err(ApiError::Forbidden(
            "JWT callers may only update their own peer record".to_string(),
        ));
    } else if auth.jwt_claims.is_none() && !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "Admin role required to update another agent's peer record".to_string(),
        ));
    }

    state
        .network_peers
        .update_endpoint(&Id::new(&id), &req.endpoint)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/network/peers/{id} — deregister a peer.
///
/// JWT callers may only delete their own peer. Admin can delete any.
pub async fn delete_peer(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Fetch peer to check ownership.
    let peers = state.network_peers.list().await?;
    let peer = peers
        .into_iter()
        .find(|p| p.id.as_str() == id)
        .ok_or_else(|| ApiError::NotFound(format!("peer {id} not found")))?;

    if auth.jwt_claims.is_some() && auth.agent_id != peer.agent_id.as_str() {
        return Err(ApiError::Forbidden(
            "JWT callers may only delete their own peer record".to_string(),
        ));
    } else if auth.jwt_claims.is_none() && !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "Admin role required to delete another agent's peer record".to_string(),
        ));
    }

    state.network_peers.delete(&Id::new(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/network/derp-map — return DERP relay map (M26.3).
///
/// Reads GYRE_DERP_SERVERS env var: JSON array of DERP region objects:
/// `[{"region_id":1,"region_name":"us-east","nodes":[{"name":"n1","host_name":"derp.example.com","ipv4":"1.2.3.4","stun_port":3478,"derp_port":443}]}]`
/// If not configured, returns a stub with the server's own base URL.
pub async fn derp_map(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Try GYRE_DERP_SERVERS first.
    if let Ok(derp_json) = std::env::var("GYRE_DERP_SERVERS") {
        if let Ok(regions_arr) = serde_json::from_str::<serde_json::Value>(&derp_json) {
            if let Some(arr) = regions_arr.as_array() {
                let mut regions = serde_json::Map::new();
                for region in arr {
                    if let Some(id) = region.get("region_id").and_then(|v| v.as_u64()) {
                        regions.insert(id.to_string(), region.clone());
                    }
                }
                return Json(serde_json::json!({ "regions": regions }));
            }
        }
    }

    // Fall back to GYRE_DERP_URL (single relay URL convenience var).
    let derp_url = std::env::var("GYRE_DERP_URL").unwrap_or_else(|_| state.base_url.clone());

    // Extract hostname from URL.
    let hostname = derp_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("localhost")
        .to_string();

    Json(serde_json::json!({
        "regions": {
            "1": {
                "regionID": 1,
                "regionCode": "gyre-default",
                "regionName": "Gyre Default DERP",
                "nodes": [
                    {
                        "name": "gyre-derp-1",
                        "regionID": 1,
                        "hostName": hostname,
                        "stunPort": 3478,
                        "derpPort": 443
                    }
                ]
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

    // A valid 44-char base64 Curve25519 pubkey (32 bytes).
    const VALID_PUBKEY: &str = "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";

    #[test]
    fn pubkey_validation_valid_44_char() {
        assert!(super::validate_wg_pubkey(VALID_PUBKEY));
    }

    #[test]
    fn pubkey_validation_valid_43_char_no_padding() {
        // 43-char variant (drop trailing '=')
        let no_pad = &VALID_PUBKEY[..43];
        assert!(super::validate_wg_pubkey(no_pad));
    }

    #[test]
    fn pubkey_validation_too_short() {
        assert!(!super::validate_wg_pubkey("abc123=="));
        assert!(!super::validate_wg_pubkey(""));
    }

    #[test]
    fn pubkey_validation_invalid_chars() {
        // 44 chars but with invalid char '@'
        assert!(!super::validate_wg_pubkey(
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuF@="
        ));
    }

    #[test]
    fn pubkey_validation_wrong_decoded_length() {
        // 44 'A' characters with no padding: 11 groups × 3 bytes = 33 bytes decoded, not 32.
        assert!(!super::validate_wg_pubkey(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
    }

    #[tokio::test]
    async fn register_peer_valid_pubkey() {
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "wireguard_pubkey": VALID_PUBKEY,
            "endpoint": "10.0.0.1:51820",
            "allowed_ips": ["10.100.0.1/32"]
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["agent_id"], "agent-1");
        assert_eq!(json["wireguard_pubkey"], VALID_PUBKEY);
        assert!(json["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn register_peer_invalid_pubkey_rejected() {
        let body = serde_json::json!({
            "agent_id": "agent-bad",
            "wireguard_pubkey": "tooshort",
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_peers_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers")
                    .header("Authorization", "Bearer test-token")
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
            "wireguard_pubkey": VALID_PUBKEY,
            "allowed_ips": []
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
            "wireguard_pubkey": VALID_PUBKEY,
            "allowed_ips": ["10.0.0.1/32"]
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers/agent/agent-lookup")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["wireguard_pubkey"], VALID_PUBKEY);
    }

    #[tokio::test]
    async fn delete_peer_no_content() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-del",
            "wireguard_pubkey": VALID_PUBKEY,
            "allowed_ips": []
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["regions"].is_object());
        // Default stub should have at least one region with a non-empty nodes array.
        let regions = json["regions"].as_object().unwrap();
        assert!(!regions.is_empty());
        let first_region = regions.values().next().unwrap();
        let nodes = first_region["nodes"].as_array().unwrap();
        assert!(
            !nodes.is_empty(),
            "DERP map should include at least one relay node"
        );
    }

    #[tokio::test]
    async fn list_peers_includes_mesh_ip_field() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-mesh",
            "wireguard_pubkey": VALID_PUBKEY,
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/network/peers")
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/network/peers/agent/agent-mesh")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // mesh_ip field is present in response (may be null if WG disabled)
        assert!(json.get("mesh_ip").is_some());
        assert!(json.get("is_stale").is_some());
    }
}
