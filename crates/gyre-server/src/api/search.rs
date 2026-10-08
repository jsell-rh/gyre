//! GET /api/v1/search — full-text search across all entities.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
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
    let started = std::time::Instant::now();
    let results = state
        .search
        .search(SearchQuery {
            query: q.clone(),
            entity_type: params.entity_type.clone(),
            workspace_id: params.workspace_id.clone(),
            limit,
        })
        .await
        .map_err(ApiError::Internal)?;
    let duration_ms = started.elapsed().as_millis() as u64;

    // Auto-track search execution (analytics.md §Auto-Emitted Events).
    {
        let entity_types: Vec<String> = {
            let mut types: Vec<String> =
                results.iter().map(|r| r.entity_type.clone()).collect();
            types.sort();
            types.dedup();
            types
        };
        let ev = gyre_domain::AnalyticsEvent::new(
            gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            "search.query",
            None,
            serde_json::json!({
                "query_length": q.chars().count(),
                "entity_types": entity_types,
                "result_count": results.len(),
                "duration_ms": duration_ms,
            }),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .with_scope(
            None,
            None,
            params.workspace_id.as_deref().map(Id::new).as_ref(),
            None,
        );
        let _ = state.analytics.record(&ev).await;
    }

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
