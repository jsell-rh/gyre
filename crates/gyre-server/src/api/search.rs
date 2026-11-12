//! GET /api/v1/search — full-text search across all entities.
//! GET /api/v1/search/suggest — autocomplete suggestions (spec §API).
//! GET /api/v1/search/facets — available facets and their value counts (§API).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_ports::search::{FacetCounts, SearchQuery};
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

fn default_suggest_limit() -> usize {
    10
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
    /// Aggregated facet counts over the filtered match set (§Response Format):
    /// `{"type": {"spec": 5, ...}, "workspace": {...}, "status": {...}}`.
    pub facet_counts: FacetCounts,
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
            facet_counts: FacetCounts::new(),
        }));
    }
    let limit = params.limit.min(100);
    let port_query = SearchQuery {
        query: q.clone(),
        entity_type: params.entity_type,
        workspace_id: params.workspace_id,
        limit,
    };
    let results = state
        .search
        .search(port_query.clone())
        .await
        .map_err(ApiError::Internal)?;

    // §Response Format: facet_counts aggregate the query's filtered match set
    // (same terms + filters), not the truncated result page and not the whole
    // index — hence a second aggregation call with the limit ignored.
    let facet_counts = state
        .search
        .facet_counts(Some(port_query))
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
        facet_counts,
    }))
}

/// GET /api/v1/search/suggest — autocomplete suggestions (spec §API).
#[derive(Deserialize)]
pub struct SuggestParams {
    pub q: Option<String>,
    #[serde(default = "default_suggest_limit")]
    pub limit: usize,
}

#[derive(Serialize)]
pub struct SuggestResponse {
    pub suggestions: Vec<gyre_ports::search::SearchSuggestion>,
}

pub async fn suggest_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SuggestParams>,
) -> Result<Json<SuggestResponse>, ApiError> {
    let q = params.q.unwrap_or_default();
    // Empty prefix matches everything — autocomplete wants nothing instead.
    if q.trim().is_empty() {
        return Ok(Json(SuggestResponse {
            suggestions: vec![],
        }));
    }
    let limit = params.limit.min(50);
    let suggestions = state
        .search
        .suggest(&q, limit)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(SuggestResponse { suggestions }))
}

/// GET /api/v1/search/facets — available facets and their value counts (§API).
#[derive(Serialize)]
pub struct FacetsResponse {
    pub facets: FacetCounts,
}

pub async fn facets_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FacetsResponse>, ApiError> {
    // No filter: aggregate the entire index.
    let facets = state
        .search
        .facet_counts(None)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(FacetsResponse { facets }))
}

/// POST /api/v1/admin/search/reindex — force full reindex (Admin only).
/// Rebuilds the entire index from the domain repositories (specs, tasks, MRs,
/// reviews, commits, agents, personas, activity events, audit events, files)
/// and returns the true number of documents indexed.
pub async fn reindex_handler(
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let count = crate::search_indexer::reindex_all(&state)
        .await
        .map_err(ApiError::Internal)?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "indexed": count })),
    ))
}
