use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use tracing::instrument;

/// GET /health - returns {"status":"ok","version":"0.1.0"}
#[instrument]
pub async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "version": "0.1.0"})),
    )
}

/// GET /healthz - Kubernetes-style liveness probe.
/// Returns 200 if the server process is running.
pub async fn healthz_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "checks": {
                "server": "ok"
            }
        })),
    )
}

/// GET /readyz - Kubernetes-style readiness probe.
/// Returns 200 if the server is ready to accept traffic (DB connected, migrations applied).
/// Since gyre-server uses in-memory repositories, readiness is equivalent to liveness.
pub async fn readyz_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "checks": {
                "server": "ok",
                "storage": "ok"
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_returns_200() {
        let app = Router::new().route("/health", get(health_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_returns_correct_json() {
        let app = Router::new().route("/health", get(health_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }

    #[tokio::test]
    async fn healthz_returns_200_with_checks() {
        let app = Router::new().route("/healthz", get(healthz_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["checks"]["server"].is_string());
    }

    #[tokio::test]
    async fn readyz_returns_200_with_checks() {
        let app = Router::new().route("/readyz", get(readyz_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["checks"]["storage"].is_string());
    }
}
