//! Helper functions for creating notifications (HSI §2).

use crate::AppState;
use gyre_common::{Id, Notification, NotificationType};

/// Create and persist a notification for a specific user.
/// Best-effort: logs errors but never panics (fire-and-forget for non-priority-1 paths).
pub async fn notify(
    state: &AppState,
    workspace_id: Id,
    user_id: Id,
    notification_type: NotificationType,
    title: impl Into<String>,
    tenant_id: impl Into<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let notif = Notification::new(
        id,
        workspace_id,
        user_id,
        notification_type,
        title,
        tenant_id,
        now,
    );

    if let Err(e) = state.notifications.create(&notif).await {
        tracing::warn!("Failed to create notification: {e}");
    }
}

/// Create and persist a notification with structured body and entity references.
/// The `body` is a JSON string that the UI parses for quick-link buttons (agent_id, mr_id, spec_path).
pub async fn notify_rich(
    state: &AppState,
    workspace_id: Id,
    user_id: Id,
    notification_type: NotificationType,
    title: impl Into<String>,
    tenant_id: impl Into<String>,
    body: Option<String>,
    entity_ref: Option<String>,
    repo_id: Option<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut notif = Notification::new(
        id,
        workspace_id,
        user_id,
        notification_type,
        title,
        tenant_id,
        now,
    );
    notif.body = body;
    notif.entity_ref = entity_ref;
    notif.repo_id = repo_id;

    if let Err(e) = state.notifications.create(&notif).await {
        tracing::warn!("Failed to create notification: {e}");
    }
}

/// Notify the spawning user that a gate failed on their MR.
///
/// Priority 3 (GateFailure) — created synchronously by the gate evaluation handler per HSI §2.
pub async fn notify_gate_failure(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    gate_name: &str,
    tenant_id: &str,
) {
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

    // Resolve MR title for human-friendly notification
    let mr_label = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());

    let body_json = serde_json::json!({
        "gate_name": gate_name,
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
    })
    .to_string();

    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::GateFailure,
        format!("Gate '{gate_name}' failed on MR {mr_label}"),
        tenant_id,
        Some(body_json),
        Some(mr_id.to_string()),
        None,
    )
    .await;
}

/// Notify the spawning user that their MR was merged.
pub async fn notify_mr_merged(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    tenant_id: &str,
) {
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

    // Resolve MR title for human-friendly notification
    let mr_label = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());

    let body_json = serde_json::json!({
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
    })
    .to_string();

    // Use SuggestedSpecLink as a low-priority informational notification.
    // A dedicated MrMerged type can be added to NotificationType when needed.
    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::SuggestedSpecLink,
        format!("MR {mr_label} was merged"),
        tenant_id,
        Some(body_json),
        Some(mr_id.to_string()),
        None,
    )
    .await;
}
