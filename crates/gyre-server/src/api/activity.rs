use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

#[derive(Deserialize)]
pub struct ActivityQueryParams {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/activity — unified activity feed.
///
/// Synthesizes activity events from notifications (which are populated for all
/// key lifecycle events: agent completions, gate failures, spec approvals, MR
/// merges, etc.). Returns a generic JSON array consumed by the dashboard.
pub async fn activity_handler(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<serde_json::Value>> {
    let lim = params.limit.unwrap_or(30);

    let notifs = state
        .notifications
        .list_recent(lim)
        .await
        .unwrap_or_default();

    let events: Vec<serde_json::Value> = notifs
        .into_iter()
        .map(|n| {
            let body: serde_json::Value = n
                .body
                .as_ref()
                .and_then(|b| serde_json::from_str(b).ok())
                .unwrap_or(serde_json::Value::Null);
            let event_type = match n.notification_type.as_str() {
                "AgentCompleted" => "agent_completed",
                "AgentFailed" => "agent_failed",
                "MrMerged" => "merged",
                "MrCreated" => "mr_created",
                "SpecApproved" => "spec_approved",
                "SpecRejected" => "spec_rejected",
                "GateFailure" => "gate_failed",
                "TaskCreated" => "task_created",
                other => other,
            };
            serde_json::json!({
                "event_type": event_type,
                "title": n.title,
                "description": n.body,
                "entity_type": if body.get("mr_id").is_some() { Some("mr") }
                    else if body.get("agent_id").is_some() { Some("agent") }
                    else if body.get("spec_path").is_some() { Some("spec") }
                    else if body.get("task_id").is_some() { Some("task") }
                    else { None },
                "entity_id": body.get("mr_id")
                    .or_else(|| body.get("agent_id"))
                    .or_else(|| body.get("spec_path"))
                    .or_else(|| body.get("task_id"))
                    .and_then(|v| v.as_str())
                    .or(n.entity_ref.as_deref()),
                "entity_name": body.get("mr_title")
                    .or_else(|| body.get("agent_name"))
                    .and_then(|v| v.as_str()),
                "timestamp": n.created_at,
                "agent_id": body.get("agent_id").and_then(|v| v.as_str()),
                "mr_id": body.get("mr_id").and_then(|v| v.as_str()),
                "task_id": body.get("task_id").and_then(|v| v.as_str()),
                "spec_path": body.get("spec_path").and_then(|v| v.as_str()),
                "repo_id": n.repo_id,
            })
        })
        .collect();

    Json(events)
}
