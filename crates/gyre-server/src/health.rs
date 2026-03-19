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
}
