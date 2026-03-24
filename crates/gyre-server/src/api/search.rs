//! GET /api/v1/search — full-text search across all entities.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_ports::search::SearchQuery;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

use crate::AppState;

use super::error::ApiError;

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub entity_type: Option<String>,
    pub workspace_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
pub struct SearchResultItem {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub facets: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: usize,
    pub results: Vec<SearchResultItem>,
}

pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let q = params.q.unwrap_or_default();
    if q.trim().is_empty() {
        return Ok(Json(SearchResponse {
            query: q,
            total: 0,
            results: vec![],
        }));
    }
    let limit = params.limit.min(100);
    let results = state
        .search
        .search(SearchQuery {
            query: q.clone(),
            entity_type: params.entity_type,
            workspace_id: params.workspace_id,
            limit,
        })
        .await
        .map_err(ApiError::Internal)?;

    let total = results.len();
    let items = results
        .into_iter()
        .map(|r| SearchResultItem {
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            title: r.title,
            snippet: r.snippet,
            score: r.score,
            facets: r.facets,
        })
        .collect();

    Ok(Json(SearchResponse {
        query: q,
        total,
        results: items,
    }))
}

/// POST /api/v1/search/reindex — force full reindex (Admin only).
pub async fn reindex_handler(
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let count = state
        .search
        .reindex_all()
        .await
        .map_err(ApiError::Internal)?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "indexed": count })),
    ))
}
