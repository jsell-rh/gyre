use std::sync::Arc;
use tracing::{error, info, warn};

use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, MergeQueueEntryStatus, MergeResult, MrStatus};
use uuid::Uuid;

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

/// Check if all `depends_on` for the given MR have status `Merged`.
/// Returns `Ok(true)` if all dependencies are satisfied, `Ok(false)` if any are pending.
async fn dependencies_satisfied(state: &AppState, mr_id: &Id) -> anyhow::Result<bool> {
    let mr = match state.merge_requests.find_by_id(mr_id).await? {
        Some(m) => m,
        None => return Ok(true), // MR not found — let the processor handle the error
    };
    for dep_id in &mr.depends_on {
        match state.merge_requests.find_by_id(dep_id).await? {
            Some(dep) if dep.status == MrStatus::Merged => continue,
            Some(_) => return Ok(false), // dependency not yet merged
            None => return Ok(false),    // missing dep — block until resolved
        }
    }
    Ok(true)
}

/// Check if all members of an atomic group are ready (all gates passed, deps satisfied).
/// Returns `Ok(true)` if no group or all members are ready.
async fn atomic_group_ready(state: &AppState, group: &str, mr_id: &Id) -> anyhow::Result<bool> {
    let all_mrs = state.merge_requests.list().await?;
    let members: Vec<_> = all_mrs
        .iter()
        .filter(|m| m.atomic_group.as_deref() == Some(group))
        .collect();

    for member in &members {
        if member.id == *mr_id {
            continue; // the current MR is handled by the caller
        }
        // Check deps satisfied for each group member
        if !dependencies_satisfied(state, &member.id).await? {
            return Ok(false);
        }
        // Check if the member is in the queue and gates are ready
        match crate::gate_executor::check_gates_for_mr(state, &member.id).await {
            Ok(true) => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

async fn process_next(state: &AppState) -> anyhow::Result<()> {
    // Get all queued entries and find the first one whose dependencies are all merged.
    let all_queued = state.merge_queue.list_queue().await?;
    let entry = {
        let mut found = None;
        for candidate in all_queued {
            if candidate.status != MergeQueueEntryStatus::Queued {
                continue;
            }
            if dependencies_satisfied(state, &candidate.merge_request_id).await? {
                found = Some(candidate);
                break;
            }
        }
        match found {
            Some(e) => e,
            None => return Ok(()),
        }
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

    // If this MR is part of an atomic group, ensure all group members are ready.
    if let Some(ref group) = mr.atomic_group {
        match atomic_group_ready(state, group, &mr.id).await {
            Ok(true) => {}
            Ok(false) => {
                info!(entry_id = %entry.id, group = %group, "atomic group not ready, requeueing");
                state
                    .merge_queue
                    .update_status(&entry.id, MergeQueueEntryStatus::Queued, None)
                    .await?;
                return Ok(());
            }
            Err(e) => {
                warn!(entry_id = %entry.id, error = %e, "error checking atomic group");
            }
        }
    }

    // Check quality gates before merging.
    match crate::gate_executor::check_gates_for_mr(state, &mr.id).await {
        Ok(true) => {} // all passed or no gates
        Ok(false) => {
            // Gates still running — put the entry back to Queued to retry later.
            info!(entry_id = %entry.id, "quality gates still running, requeueing");
            state
                .merge_queue
                .update_status(&entry.id, MergeQueueEntryStatus::Queued, None)
                .await?;
            return Ok(());
        }
        Err(reason) => {
            warn!(entry_id = %entry.id, reason = %reason, "quality gate failed, blocking merge");
            state
                .merge_queue
                .update_status(
                    &entry.id,
                    MergeQueueEntryStatus::Failed,
                    Some(format!("quality gate failed: {reason}")),
                )
                .await?;
            return Ok(());
        }
    }

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

            // Auto-track merge_queue.processed analytics event
            let ev = AnalyticsEvent::new(
                Id::new(Uuid::new_v4().to_string()),
                "merge_queue.processed",
                updated_mr.author_agent_id.as_ref().map(|id| id.to_string()),
                serde_json::json!({
                    "entry_id": entry.id.to_string(),
                    "mr_id": updated_mr.id.to_string(),
                    "result": "merged",
                }),
                now,
            );
            let _ = state.analytics.record(&ev).await;
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
