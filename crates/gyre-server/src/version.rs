use axum::{response::IntoResponse, Json};
use serde_json::json;
use tracing::instrument;

/// GET /api/v1/version - returns gyre server version info.
#[instrument]
pub async fn version_handler() -> impl IntoResponse {
    Json(json!({
        "name": "gyre",
        "version": "0.1.0",
        "milestone": "M0"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn version_returns_200() {
        let app = Router::new().route("/api/v1/version", get(version_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn version_returns_correct_json() {
        let app = Router::new().route("/api/v1/version", get(version_handler));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "gyre");
        assert_eq!(json["version"], "0.1.0");
        assert_eq!(json["milestone"], "M0");
    }
}
