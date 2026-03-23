//! Stale WireGuard peer detection: marks peers inactive when heartbeat TTL expires (M26.4).

use std::sync::Arc;
use tracing::{debug, error};

use crate::AppState;

/// Run one stale-peer detection cycle.
/// Peers whose `last_seen` (or `registered_at` if never seen) is older than
/// `GYRE_WG_PEER_TTL` seconds are marked stale and excluded from peer distribution.
pub async fn run_once(state: &AppState) -> anyhow::Result<usize> {
    let ttl = state.wg_config.peer_ttl_secs;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now.saturating_sub(ttl);
    let marked = state.network_peers.mark_stale_older_than(cutoff).await?;
    if marked > 0 {
        debug!(count = marked, "marked WireGuard peers as stale");
    }
    Ok(marked)
}

pub fn spawn_stale_peer_detector(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Check every 60 seconds.
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = run_once(&state).await {
                error!("stale peer check failed: {e}");
            }
        }
    });
}
