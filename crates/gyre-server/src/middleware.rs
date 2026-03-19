//! HTTP request tracing middleware.
//!
//! For every request:
//! - Generates a UUID request ID and attaches it to the tracing span.
//! - Records method, path, status_code, duration_ms on the span.
//! - Adds `X-Request-Id` response header.
//! - Updates Prometheus request counter and duration histogram.

use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use std::sync::Arc;
use tracing::Instrument;

use crate::AppState;

pub async fn request_tracing(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let method = req.method().to_string();
    // Normalise path to avoid high cardinality in metrics (strip query string).
    let path = req.uri().path().to_string();

    let span = tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        request_id = %request_id,
        status_code = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    );

    async move {
        let start = std::time::Instant::now();
        let mut response = next.run(req).await;
        let elapsed = start.elapsed();
        let duration_secs = elapsed.as_secs_f64();
        let status = response.status().as_u16().to_string();

        tracing::Span::current().record("status_code", &status as &str);
        tracing::Span::current().record("duration_ms", elapsed.as_millis());

        // Update Prometheus metrics.
        state
            .metrics
            .http_requests_total
            .with_label_values(&[&method, &path, &status])
            .inc();
        state
            .metrics
            .http_request_duration_seconds
            .with_label_values(&[&method, &path])
            .observe(duration_secs);

        // Attach request ID to the response.
        if let Ok(val) = request_id.parse() {
            response.headers_mut().insert("x-request-id", val);
        }

        response
    }
    .instrument(span)
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = test_state();
        crate::build_router(state)
    }

    #[tokio::test]
    async fn request_id_header_present() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers().contains_key("x-request-id"),
            "missing X-Request-Id header"
        );
    }

    #[tokio::test]
    async fn request_id_is_uuid_format() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = resp
            .headers()
            .get("x-request-id")
            .expect("X-Request-Id must be present")
            .to_str()
            .expect("X-Request-Id must be ASCII");
        // UUID format: 8-4-4-4-12 hex digits.
        assert_eq!(id.len(), 36, "request id length: {id}");
        assert!(id.contains('-'), "request id should be UUID: {id}");
    }

    #[tokio::test]
    async fn request_id_unique_per_request() {
        let app = app();
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id1 = r1.headers().get("x-request-id").unwrap().to_str().unwrap();
        let id2 = r2.headers().get("x-request-id").unwrap().to_str().unwrap();
        assert_ne!(id1, id2, "request IDs should be unique per request");
    }

    #[tokio::test]
    async fn metrics_endpoint_returns_200() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint_is_prometheus_format() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(
            text.contains("# HELP") || text.contains("# TYPE"),
            "metrics response is not Prometheus format: {text}"
        );
    }
}
