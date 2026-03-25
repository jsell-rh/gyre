use axum::{
    extract::{Query, State},
    Json,
};
use gyre_common::ActivityEventData;
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::AppState;

#[derive(Deserialize)]
pub struct ActivityQueryParams {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/activity — legacy global activity endpoint backed by TelemetryBuffer.
///
/// Scoped to workspaces belonging to the caller's tenant. Admin callers
/// (global token) see all workspaces.
pub async fn activity_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<ActivityEventData>> {
    let since_ms = params.since.unwrap_or(0);
    let lim = params.limit.unwrap_or(100);

    let msgs = if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        // Admin: query all workspaces (backward compat for global token).
        state.telemetry_buffer.list_all_since(since_ms, lim)
    } else {
        // Scoped: only workspaces belonging to caller's tenant.
        let workspaces = state
            .workspaces
            .list_by_tenant(&gyre_common::Id::new(&auth.tenant_id))
            .await
            .unwrap_or_default();
        let mut msgs = vec![];
        for ws in &workspaces {
            let ws_msgs =
                state
                    .telemetry_buffer
                    .list_since(&ws.id, since_ms, lim.saturating_sub(msgs.len()));
            msgs.extend(ws_msgs);
            if msgs.len() >= lim {
                break;
            }
        }
        msgs
    };

    let events: Vec<ActivityEventData> = msgs
        .into_iter()
        .filter_map(|m| {
            m.payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
        })
        .collect();
    Json(events)
}
