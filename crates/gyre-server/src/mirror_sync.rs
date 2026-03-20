//! Background mirror sync job.
//!
//! Runs every 60 seconds and calls `git fetch --all` on every repository
//! where `is_mirror == true`, then updates `last_mirror_sync`.

use crate::api::now_secs;
use crate::AppState;
use std::sync::Arc;
use tracing::{error, info};

pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let repos = state.repos.list().await?;
    let mirrors: Vec<_> = repos.into_iter().filter(|r| r.is_mirror).collect();

    if mirrors.is_empty() {
        return Ok(());
    }

    info!(count = mirrors.len(), "mirror_sync: syncing mirrors");

    for mut repo in mirrors {
        match state.git_ops.fetch_mirror(&repo.path).await {
            Ok(()) => {
                repo.last_mirror_sync = Some(now_secs());
                if let Err(e) = state.repos.update(&repo).await {
                    error!(repo_id = %repo.id, "mirror_sync: failed to update last_mirror_sync: {e}");
                } else {
                    info!(repo_id = %repo.id, path = %repo.path, "mirror_sync: synced");
                }
            }
            Err(e) => {
                error!(repo_id = %repo.id, path = %repo.path, "mirror_sync: fetch failed: {e}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn run_once_with_no_mirrors_succeeds() {
        let state = test_state();
        run_once(&state).await.unwrap();
    }
}
