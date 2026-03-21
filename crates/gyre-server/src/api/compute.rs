use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

// ── Request / Response ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateComputeTargetRequest {
    pub name: String,
    pub target_type: String,
    pub config: Option<serde_json::Value>,
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
}
