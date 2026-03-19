use std::sync::Arc;
use tracing::{error, info, warn};

use gyre_domain::{MergeQueueEntryStatus, MergeResult, MrStatus};

use crate::AppState;

pub fn spawn_merge_processor(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(e) = process_next(&state).await {
                error!("merge processor error: {e:#}");
            }
        }
    });
}

/// Run one merge-processor cycle. Used by the job framework for manual triggering.
pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    process_next(state).await
}

async fn process_next(state: &AppState) -> anyhow::Result<()> {
    let entry = match state.merge_queue.next_pending().await? {
        Some(e) => e,
        None => return Ok(()),
    };

    info!(entry_id = %entry.id, mr_id = %entry.merge_request_id, "processing merge queue entry");

    // Mark as Processing
    state
        .merge_queue
        .update_status(&entry.id, MergeQueueEntryStatus::Processing, None)
        .await?;

    // Look up the merge request
    let mr = match state
        .merge_requests
        .find_by_id(&entry.merge_request_id)
        .await?
    {
        Some(mr) => mr,
        None => {
            warn!(mr_id = %entry.merge_request_id, "MR not found for queue entry");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some("merge request not found".to_string()),
                )
                .await?;
            return Ok(());
        }
    };

    // Look up the repository
    let repo = match state.repos.find_by_id(&mr.repository_id).await? {
        Some(r) => r,
        None => {
            warn!(repo_id = %mr.repository_id, "repository not found for MR");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some("repository not found".to_string()),
                )
                .await?;
            return Ok(());
        }
    };

    // Attempt the merge
    let result = state
        .git_ops
        .merge_branches(&repo.path, &mr.source_branch, &mr.target_branch)
        .await;

    match result {
        Ok(MergeResult::Success { merge_commit_sha }) => {
            info!(entry_id = %entry.id, sha = %merge_commit_sha, "merge succeeded");

            // Update MR status to Merged
            let mut updated_mr = mr;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            updated_mr.updated_at = now;
            // Transition: Approved -> Merged. If already Open, transition to Approved first.
            if updated_mr.status == MrStatus::Open {
                let _ = updated_mr.transition_status(MrStatus::Approved);
            }
            if let Err(e) = updated_mr.transition_status(MrStatus::Merged) {
                warn!("could not transition MR to Merged: {e}");
            }
            let _ = state.merge_requests.update(&updated_mr).await;

            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Merged, None)
                .await?;
        }
        Ok(MergeResult::Conflict { message }) => {
            warn!(entry_id = %entry.id, reason = %message, "merge conflict");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("conflict: {}", message)),
                )
                .await?;
        }
        Err(e) => {
            error!(entry_id = %entry.id, error = %e, "git merge error");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("git error: {e}")),
                )
                .await?;
        }
    }

    Ok(())
}
