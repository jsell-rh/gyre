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

    info!("gyre CLI starting");

    // TODO(m0.3): wire up clap commands, WebSocket connection to server, TUI
    // See: specs/milestones/m0-walking-skeleton.md - Deliverable 3: CLI Connects

    Ok(())
}
