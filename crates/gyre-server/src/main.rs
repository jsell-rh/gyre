mod activity;
mod health;
mod spa;
mod ws;

use anyhow::Result;
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use gyre_adapters::sqlite::SqliteStorage;
use gyre_common::ActivityEventData;
use gyre_ports::storage::StoragePort;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

/// Shared application state available to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub auth_token: String,
    pub activity: activity::ActivityStore,
    pub broadcast_tx: broadcast::Sender<ActivityEventData>,
}

/// Build the axum Router (extracted for testability).
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health::health_handler))
        .route("/ws", get(ws::ws_handler))
        .route("/api/activity", get(activity_query_handler))
        .route("/", get(spa::spa_handler))
        .route("/*path", get(spa::spa_handler))
        .with_state(state)
}

#[derive(Deserialize)]
struct ActivityQueryParams {
    since: Option<u64>,
    limit: Option<usize>,
}

async fn activity_query_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<ActivityEventData>> {
    Json(state.activity.query(params.since, params.limit))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("gyre-server starting");

    let port: u16 = std::env::var("GYRE_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?;

    let auth_token =
        std::env::var("GYRE_AUTH_TOKEN").unwrap_or_else(|_| "gyre-dev-token".to_string());

    let db_path = std::env::var("GYRE_DB_PATH").unwrap_or_else(|_| "gyre.db".to_string());

    // Initialize SQLite storage and verify connectivity.
    let storage = tokio::task::spawn_blocking(move || SqliteStorage::new(&db_path)).await??;
    storage.health_check().await?;
    info!("storage healthy");

    let (broadcast_tx, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        auth_token,
        activity: activity::ActivityStore::new(),
        broadcast_tx,
    });
    let app = build_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("gyre-server stopped");
    Ok(())
}

/// Wait for SIGINT or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to listen for ctrl-c");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_app() -> Router {
        let (broadcast_tx, _) = broadcast::channel(16);
        let state = Arc::new(AppState {
            auth_token: "test-token".to_string(),
            activity: activity::ActivityStore::new(),
            broadcast_tx,
        });
        build_router(state)
    }

    #[tokio::test]
    async fn integration_health_endpoint() {
        let app = test_app();
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

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }
}
