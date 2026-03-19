use anyhow::Result;
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

    // TODO(m0.2): wire up axum HTTP server, WebSocket endpoint, health check
    // See: specs/milestones/m0-walking-skeleton.md - Deliverable 2: Server Boots

    Ok(())
}
