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
                let now = now_secs();
                repo.last_mirror_sync = Some(now);
                if let Err(e) = state.repos.update(&repo).await {
                    error!(repo_id = %repo.id, "mirror_sync: failed to update last_mirror_sync: {e}");
                } else {
                    info!(repo_id = %repo.id, path = %repo.path, "mirror_sync: synced");
                }

                // Run the same post-receive processing as git push:
                // spec ledger sync, knowledge graph extraction, dependency detection.
                let repo_id_str = repo.id.to_string();
                let workspace_id_str = repo.workspace_id.to_string();
                let default_branch = repo.default_branch.clone();
                let default_ref = format!("refs/heads/{default_branch}");
                let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

                // Resolve HEAD SHA of the default branch after fetch.
                if let Ok(output) = tokio::process::Command::new(&git_bin)
                    .args(["-C", &repo.path, "rev-parse", &default_ref])
                    .output()
                    .await
                {
                    if output.status.success() {
                        let new_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

                        // Spec ledger sync.
                        let workspace_tenant_id = state
                            .workspaces
                            .find_by_id(&repo.workspace_id)
                            .await
                            .ok()
                            .flatten()
                            .map(|ws| ws.tenant_id);
                        crate::spec_registry::sync_spec_ledger(
                            &state.spec_ledger,
                            &state.spec_links_store,
                            &repo.path,
                            &new_sha,
                            now,
                            Some(&repo_id_str),
                            Some(&workspace_id_str),
                            Some(&state.workspaces),
                            Some(&state.repos),
                            workspace_tenant_id.as_ref(),
                            Some(&state.tasks),
                        )
                        .await;

                        // Knowledge graph extraction.
                        crate::graph_extraction::extract_and_store_graph(
                            &repo.path,
                            &repo_id_str,
                            &new_sha,
                            Arc::clone(&state.graph_store),
                            &git_bin,
                            None, // no agent context for mirror syncs
                            None, // no divergence check
                        )
                        .await;

                        info!(repo_id = %repo.id, "mirror_sync: post-sync processing complete (spec ledger + graph)");
                    }
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
