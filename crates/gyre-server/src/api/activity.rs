use axum::{
    extract::{Query, State},
    Json,
};
use gyre_common::ActivityEventData;
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Deserialize)]
pub struct ActivityQueryParams {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/activity — legacy global activity endpoint backed by TelemetryBuffer.
pub async fn activity_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<ActivityEventData>> {
    let since_ms = params.since.unwrap_or(0);
    let lim = params.limit.unwrap_or(100);
    let msgs = state.telemetry_buffer.list_all_since(since_ms, lim);
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
