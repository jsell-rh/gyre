//! Helper functions for creating notifications (M22.8).

use crate::AppState;
use gyre_common::Id;
use gyre_domain::{Notification, NotificationPriority, NotificationType};

/// Create and persist a notification for a specific user. Best-effort: logs errors, never panics.
pub async fn notify(
    state: &AppState,
    user_id: Id,
    notification_type: NotificationType,
    title: impl Into<String>,
    body: impl Into<String>,
    priority: NotificationPriority,
    action_url: Option<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut notif = Notification::new(id, user_id, notification_type, title, body, priority, now);
    notif.action_url = action_url;

    if let Err(e) = state.notifications.create(&notif).await {
        tracing::warn!("Failed to create notification: {e}");
    }
}

/// Notify the spawning user that an agent completed and opened an MR needing review.
pub async fn notify_mr_needs_review(state: &AppState, spawned_by: &str, mr_id: &str) {
    let user_id = Id::new(spawned_by.to_string());
    notify(
        state,
        user_id,
        NotificationType::MrNeedsReview,
        "MR ready for review",
        format!("An agent has opened MR {mr_id} for review"),
        NotificationPriority::Medium,
        Some(format!("/merge-requests/{mr_id}")),
    )
    .await;
}

/// Notify the MR author that a gate failed.
pub async fn notify_gate_failure(
    state: &AppState,
    author_agent_id: &Id,
    mr_id: &str,
    gate_name: &str,
) {
    // Look up the agent to find its spawning user.
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(ref sb) = spawned_by {
        Id::new(sb.clone())
    } else {
        author_agent_id.clone()
    };

    notify(
        state,
        user_id,
        NotificationType::GateFailure,
        format!("Gate failed: {gate_name}"),
        format!("Gate '{gate_name}' failed on MR {mr_id}"),
        NotificationPriority::High,
        Some(format!("/merge-requests/{mr_id}")),
    )
    .await;
}

/// Notify the MR author that their MR was merged.
pub async fn notify_mr_merged(state: &AppState, author_agent_id: &Id, mr_id: &str) {
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(ref sb) = spawned_by {
        Id::new(sb.clone())
    } else {
        return; // No human to notify
    };

    notify(
        state,
        user_id,
        NotificationType::MrMerged,
        "MR merged",
        format!("Your MR {mr_id} was merged"),
        NotificationPriority::Low,
        Some(format!("/merge-requests/{mr_id}")),
    )
    .await;
}
