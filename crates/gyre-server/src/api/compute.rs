use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// anyhow is available via the workspace; used only in tunnel handlers.
#[allow(unused_imports)]
use anyhow::anyhow;

use super::error::ApiError;
use super::new_id;
use crate::AppState;

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTargetConfig {
    pub id: String,
    pub name: String,
    /// "local" | "docker" | "ssh" | "container"
    pub target_type: String,
    /// Target-specific configuration (image, host, user, etc.)
    pub config: serde_json::Value,
}

/// Metadata for an active SSH tunnel.  Stored in-memory; survives until the
/// server restarts or the tunnel is explicitly closed via DELETE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelRecord {
    pub id: String,
    /// The compute target this tunnel is attached to.
    pub target_id: String,
    /// `"forward"` (`-L`) or `"reverse"` (`-R`).
    pub direction: String,
    /// Local-side port.  For forward tunnels this is where the local process
    /// listens; for reverse tunnels this is the agent-side port being exposed.
    pub local_port: u16,
    pub local_host: String,
    /// Remote-side port.  For forward tunnels this is the service on the remote
    /// host; for reverse tunnels this is the port opened on the remote (server).
    pub remote_port: u16,
    pub remote_host: String,
    /// OS PID of the `ssh -N` process.
    pub pid: Option<u32>,
    /// `"active"` while the process is running, `"closed"` after termination.
    pub status: String,
}

// ── Request / Response ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateComputeTargetRequest {
    pub name: String,
    pub target_type: String,
    pub config: Option<serde_json::Value>,
}

/// Request body for `POST /api/v1/admin/compute-targets/{id}/tunnel`.
#[derive(Deserialize)]
pub struct OpenTunnelRequest {
    /// `"forward"` or `"reverse"`.
    pub direction: String,
    pub local_port: u16,
    /// Defaults to `"localhost"` when omitted.
    pub local_host: Option<String>,
    pub remote_port: u16,
    /// Defaults to `"localhost"` when omitted.
    pub remote_host: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/admin/compute-targets
pub async fn create_compute_target(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateComputeTargetRequest>,
) -> Result<(StatusCode, Json<ComputeTargetConfig>), ApiError> {
    if !matches!(
        req.target_type.as_str(),
        "local" | "docker" | "ssh" | "container"
    ) {
        return Err(ApiError::InvalidInput(format!(
            "invalid target_type '{}'; must be local, docker, ssh, or container",
            req.target_type
        )));
    }

    let ct = ComputeTargetConfig {
        id: new_id().to_string(),
        name: req.name,
        target_type: req.target_type,
        config: req
            .config
            .unwrap_or(serde_json::Value::Object(Default::default())),
    };

    state
        .compute_targets
        .lock()
        .await
        .insert(ct.id.clone(), ct.clone());

    Ok((StatusCode::CREATED, Json(ct)))
}

/// GET /api/v1/admin/compute-targets
pub async fn list_compute_targets(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<ComputeTargetConfig>> {
    let store = state.compute_targets.lock().await;
    let mut targets: Vec<ComputeTargetConfig> = store.values().cloned().collect();
    targets.sort_by(|a, b| a.name.cmp(&b.name));
    Json(targets)
}

/// GET /api/v1/admin/compute-targets/:id
pub async fn get_compute_target(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ComputeTargetConfig>, ApiError> {
    let store = state.compute_targets.lock().await;
    store
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("compute target {id} not found")))
}

/// DELETE /api/v1/admin/compute-targets/:id
pub async fn delete_compute_target(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut store = state.compute_targets.lock().await;
    if store.remove(&id).is_none() {
        return Err(ApiError::NotFound(format!("compute target {id} not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Tunnel handlers (G12) ─────────────────────────────────────────────────────

/// POST /api/v1/admin/compute-targets/:id/tunnel
///
/// Open an SSH tunnel (forward or reverse) through an existing SSH compute
/// target.  Returns a [`TunnelRecord`] with the tunnel id and PID.
///
/// **Reverse tunnels** (`direction: "reverse"`) allow air-gapped agents to
/// phone home: the agent opens an SSH connection back to the gyre server and
/// requests that `remote_port` on the server side be forwarded to `local_port`
/// on the agent.  The server can then reach the agent through its own loopback.
///
/// **Forward tunnels** (`direction: "forward"`) expose a service running on the
/// remote SSH host as a local port on the gyre server.
///
/// Requires the compute target to have `target_type = "ssh"` and a `config`
/// object with at least `user` and `host` fields.
pub async fn open_tunnel(
    State(state): State<Arc<AppState>>,
    Path(target_id): Path<String>,
    Json(req): Json<OpenTunnelRequest>,
) -> Result<(StatusCode, Json<TunnelRecord>), ApiError> {
    // Validate direction
    if !matches!(req.direction.as_str(), "forward" | "reverse") {
        return Err(ApiError::InvalidInput(
            "direction must be 'forward' or 'reverse'".to_string(),
        ));
    }

    // Look up the compute target and verify it is SSH
    let config = {
        let store = state.compute_targets.lock().await;
        store
            .get(&target_id)
            .cloned()
            .ok_or_else(|| ApiError::NotFound(format!("compute target {target_id} not found")))?
    };

    if config.target_type != "ssh" {
        return Err(ApiError::InvalidInput(format!(
            "compute target '{}' has type '{}'; tunnels require type 'ssh'",
            target_id, config.target_type
        )));
    }

    // Build SshTarget from the config blob
    let user = config.config["user"]
        .as_str()
        .ok_or_else(|| ApiError::InvalidInput("ssh config missing 'user' field".to_string()))?
        .to_string();
    let host = config.config["host"]
        .as_str()
        .ok_or_else(|| ApiError::InvalidInput("ssh config missing 'host' field".to_string()))?
        .to_string();

    let mut ssh_target = gyre_adapters::compute::SshTarget::new(user, host);

    if let Some(identity) = config.config["identity_file"].as_str() {
        ssh_target = ssh_target.with_identity(identity);
    }
    if let Some(port) = config.config["port"].as_u64() {
        ssh_target = ssh_target.with_port(port as u16);
    }

    let local_host = req
        .local_host
        .clone()
        .unwrap_or_else(|| "localhost".to_string());
    let remote_host = req
        .remote_host
        .clone()
        .unwrap_or_else(|| "localhost".to_string());

    let kind = if req.direction == "reverse" {
        gyre_adapters::compute::TunnelKind::Reverse {
            remote_port: req.remote_port,
            local_host: local_host.clone(),
            local_port: req.local_port,
        }
    } else {
        gyre_adapters::compute::TunnelKind::Forward {
            local_port: req.local_port,
            remote_host: remote_host.clone(),
            remote_port: req.remote_port,
        }
    };

    let tunnel = ssh_target
        .open_tunnel(kind)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("failed to open SSH tunnel: {e}")))?;

    let record = TunnelRecord {
        id: tunnel.id.clone(),
        target_id: target_id.clone(),
        direction: req.direction.clone(),
        local_port: req.local_port,
        local_host,
        remote_port: req.remote_port,
        remote_host,
        pid: tunnel.pid,
        status: "active".to_string(),
    };

    // Store tunnel record; the SSH process runs independently tracked by PID.
    // When close_tunnel is called we send SIGTERM to the PID.
    // We intentionally don't hold the Child handle here — if the server
    // restarts, orphan SSH processes are cleaned up by the OS when it detects
    // the parent is gone (ssh -N exits when stdin closes).
    state
        .tunnel_store
        .lock()
        .await
        .insert(record.id.clone(), record.clone());

    Ok((StatusCode::CREATED, Json(record)))
}

/// GET /api/v1/admin/compute-targets/:id/tunnel
///
/// List all tunnels for this compute target.
pub async fn list_tunnels(
    State(state): State<Arc<AppState>>,
    Path(target_id): Path<String>,
) -> Result<Json<Vec<TunnelRecord>>, ApiError> {
    // Verify target exists
    {
        let store = state.compute_targets.lock().await;
        if !store.contains_key(&target_id) {
            return Err(ApiError::NotFound(format!(
                "compute target {target_id} not found"
            )));
        }
    }

    let tunnel_store = state.tunnel_store.lock().await;
    let mut tunnels: Vec<TunnelRecord> = tunnel_store
        .values()
        .filter(|t| t.target_id == target_id)
        .cloned()
        .collect();
    tunnels.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(tunnels))
}

/// DELETE /api/v1/admin/compute-targets/:id/tunnel/:tunnel_id
///
/// Close an active SSH tunnel.  Sends SIGTERM to the `ssh -N` process.
pub async fn close_tunnel(
    State(state): State<Arc<AppState>>,
    Path((target_id, tunnel_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mut tunnel_store = state.tunnel_store.lock().await;

    let record = tunnel_store
        .get_mut(&tunnel_id)
        .filter(|t| t.target_id == target_id)
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "tunnel {tunnel_id} not found for target {target_id}"
            ))
        })?;

    if record.status == "closed" {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Kill the ssh process by PID
    if let Some(pid) = record.pid {
        let _ = tokio::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await;
        tracing::debug!(tunnel_id = %tunnel_id, pid, "sent SIGTERM to SSH tunnel process");
    }

    record.status = "closed".to_string();
    Ok(StatusCode::NO_CONTENT)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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

    async fn create_target(
        app: Router,
        name: &str,
        target_type: &str,
    ) -> (Router, serde_json::Value) {
        let body = serde_json::json!({ "name": name, "target_type": target_type });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        (app, json)
    }

    #[tokio::test]
    async fn create_local_compute_target() {
        let (_, json) = create_target(app(), "my-local", "local").await;
        assert_eq!(json["target_type"], "local");
        assert_eq!(json["name"], "my-local");
        assert!(!json["id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_docker_compute_target_with_config() {
        let body = serde_json::json!({
            "name": "docker-runner",
            "target_type": "docker",
            "config": { "image": "ubuntu:22.04" }
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["target_type"], "docker");
        assert_eq!(json["config"]["image"], "ubuntu:22.04");
    }

    #[tokio::test]
    async fn create_ssh_compute_target_with_config() {
        let body = serde_json::json!({
            "name": "remote-host",
            "target_type": "ssh",
            "config": { "user": "ubuntu", "host": "10.0.0.5" }
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["target_type"], "ssh");
        assert_eq!(json["config"]["host"], "10.0.0.5");
    }

    #[tokio::test]
    async fn create_container_compute_target_with_config() {
        let body = serde_json::json!({
            "name": "container-runner",
            "target_type": "container",
            "config": {
                "image": "ghcr.io/my-org/gyre-agent:latest",
                "runtime": "auto",
                "volumes": ["/data:/data"],
                "env_vars": { "GYRE_SERVER_URL": "http://gyre:3000" }
            }
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["target_type"], "container");
        assert_eq!(json["config"]["image"], "ghcr.io/my-org/gyre-agent:latest");
        assert_eq!(json["config"]["runtime"], "auto");
    }

    #[tokio::test]
    async fn invalid_target_type_rejected() {
        let body = serde_json::json!({ "name": "bad", "target_type": "kubernetes" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_compute_targets_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/compute-targets")
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
    async fn list_compute_targets_after_create() {
        let app = app();
        let (app, _) = create_target(app, "t1", "local").await;
        let (app, _) = create_target(app, "t2", "docker").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/compute-targets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn get_compute_target_by_id() {
        let app = app();
        let (app, created) = create_target(app, "fetch-me", "ssh").await;
        let id = created["id"].as_str().unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/admin/compute-targets/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["id"], created["id"]);
        assert_eq!(json["name"], "fetch-me");
    }

    #[tokio::test]
    async fn get_compute_target_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/compute-targets/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_compute_target() {
        let app = app();
        let (app, created) = create_target(app, "delete-me", "local").await;
        let id = created["id"].as_str().unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/compute-targets/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Confirm gone
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/admin/compute-targets/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_nonexistent_compute_target() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/admin/compute-targets/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Tunnel tests (G12) ────────────────────────────────────────────────────

    async fn create_ssh_target(app: Router) -> (Router, String) {
        let body = serde_json::json!({
            "name": "ssh-remote",
            "target_type": "ssh",
            "config": { "user": "ubuntu", "host": "10.0.0.5" }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn open_tunnel_requires_ssh_target() {
        let app = app();
        let (app, id) = create_target(app, "local-tgt", "local").await;
        let id = id["id"].as_str().unwrap().to_string();

        let body = serde_json::json!({
            "direction": "reverse",
            "local_port": 3000,
            "remote_port": 9000,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/compute-targets/{id}/tunnel"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Local targets don't support tunnels
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn open_tunnel_invalid_direction_rejected() {
        let app = app();
        let (app, target_id) = create_ssh_target(app).await;

        let body = serde_json::json!({
            "direction": "sideways",
            "local_port": 3000,
            "remote_port": 9000,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/compute-targets/{target_id}/tunnel"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn open_tunnel_nonexistent_target_returns_404() {
        let body = serde_json::json!({
            "direction": "reverse",
            "local_port": 3000,
            "remote_port": 9000,
        });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets/no-such-target/tunnel")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_tunnels_empty_for_new_target() {
        let app = app();
        let (app, target_id) = create_ssh_target(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/admin/compute-targets/{target_id}/tunnel"))
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
    async fn list_tunnels_nonexistent_target_returns_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/compute-targets/no-such/tunnel")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn close_tunnel_nonexistent_returns_404() {
        let app = app();
        let (app, target_id) = create_ssh_target(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/admin/compute-targets/{target_id}/tunnel/no-such-tunnel"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
