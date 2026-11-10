//! GET /api/v1/search — full-text search across all entities.
//! GET /api/v1/search/suggest — autocomplete suggestions (spec §API).
//! GET /api/v1/search/facets — available facets and their value counts (§API).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
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
    let started = std::time::Instant::now();
    let port_query = SearchQuery {
        query: q.clone(),
        entity_type: params.entity_type.clone(),
        workspace_id: params.workspace_id.clone(),
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
    let duration_ms = started.elapsed().as_millis() as u64;

    // Auto-track search execution (analytics.md §Auto-Emitted Events).
    {
        let entity_types: Vec<String> = {
            let mut types: Vec<String> = results.iter().map(|r| r.entity_type.clone()).collect();
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

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::body::Body;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn search_emits_analytics_event() {
        // analytics.md §Auto-Emitted Events: search.query carries
        // query_length, entity_types, result_count, duration_ms.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Index a searchable task through the public API so the search port
        // has a real document (create_task indexes for search).
        let task_body = serde_json::json!({
            "title": "Payment reconciliation engine",
            "task_type": "implementation"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&task_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=reconciliation")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["total"], 1, "search must find the indexed task");

        let events = state
            .analytics
            .query(Some("search.query"), None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "one search.query event expected");
        let ev = &events[0];
        assert_eq!(
            ev.properties["query_length"], 14,
            "\"reconciliation\" has 14 chars"
        );
        assert_eq!(
            ev.properties["entity_types"],
            serde_json::json!(["task"]),
            "entity_types lists the distinct result types"
        );
        assert_eq!(ev.properties["result_count"], 1);
        assert!(
            ev.properties["duration_ms"].as_u64().is_some(),
            "duration_ms must be present"
        );
    }

    #[tokio::test]
    async fn empty_query_emits_no_analytics_event() {
        // Blank queries short-circuit before search — no event is recorded.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = state
            .analytics
            .query(Some("search.query"), None, 10)
            .await
            .unwrap();
        assert!(events.is_empty(), "no event for empty query");
    }
}
