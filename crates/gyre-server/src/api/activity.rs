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

pub async fn activity_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<ActivityEventData>> {
    Json(state.activity_store.query(params.since, params.limit))
}
