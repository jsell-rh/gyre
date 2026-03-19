use anyhow::Result;
use gyre_server::{build_router, build_state, merge_processor, spawn_stale_agent_detector};
use tracing::info;

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

    let base_url =
        std::env::var("GYRE_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));

    let state = build_state(&auth_token, &base_url);

    // Background tasks.
    spawn_stale_agent_detector(state.clone());
    merge_processor::spawn_merge_processor(state.clone());

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
