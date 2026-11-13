//! GET /api/v1/search — full-text search across all entities.
//! GET /api/v1/search/suggest — autocomplete suggestions (spec §API).
//! GET /api/v1/search/facets — available facets and their value counts (§API).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_ports::search::{parse_query, FacetCounts, SearchQuery};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

use crate::auth::AuthenticatedAgent;
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
    auth: AuthenticatedAgent,
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
    // Caller-scope validation (task-146 review round 2, same rule as the
    // analytics record handler): the workspace_id filter doubles as the
    // search.query event's workspace attribution — persisting it
    // unvalidated would let any authenticated caller attribute events to
    // an arbitrary workspace and pollute that scope's filtered analytics.
    // Mismatch or unknown id rejects the request.
    let workspace_scope = match params.workspace_id.as_deref() {
        Some(w) => {
            let id = Id::new(w);
            super::audit::validate_workspace_scope(&state, &id, &auth).await?;
            Some(id)
        }
        None => None,
    };
    let limit = params.limit.min(100);
    let started = std::time::Instant::now();
    // §Query Language: parse `q` (terms, quoted phrases, facets) — the
    // structured form is what the search port executes. The legacy
    // entity_type / workspace_id params remain as backward-compatible
    // shortcuts, but query-string facets take precedence: `type:` in `q`
    // overrides entity_type, `workspace:` overrides workspace_id. When the
    // facet overrides the workspace_id param, the param is dropped as a
    // filter but still validated above — it remains the search.query
    // event's workspace attribution, so a forged value is rejected before
    // the search runs.
    let parsed = parse_query(&q);
    let entity_type = if parsed.facets.contains_key("type") {
        None
    } else {
        params.entity_type.clone()
    };
    let workspace_id = if parsed.facets.contains_key("workspace") {
        None
    } else {
        workspace_scope.as_ref().map(|id| id.to_string())
    };
    // §Access Scoping: tenant/workspace/repo scope is derived from the
    // authenticated caller — never from client-supplied params. The
    // client's `workspace_id` stays a narrowing filter that the adapter
    // intersects with the accessible set: requesting an inaccessible
    // (but same-tenant) workspace yields zero results, not an error that
    // leaks its existence. Cross-tenant or unknown ids are rejected up
    // front (task-146 review round 2) because the param also attributes
    // the auto-emitted `search.query` analytics event — a forged scope
    // is Forbidden instead of polluting the victim scope's analytics.
    let scope = crate::search_scope::derive_search_scope(&state, &auth).await;
    let port_query = SearchQuery {
        query: q.clone(),
        parsed,
        entity_type,
        workspace_id,
        limit,
        tenant_id: scope.tenant_id,
        accessible_workspace_ids: scope.accessible_workspace_ids,
        accessible_repo_ids: scope.accessible_repo_ids,
    };
    let results = state
        .search
        .search(port_query.clone())
        .await
        .map_err(ApiError::Internal)?;

    // §Response Format: facet_counts aggregate the query's filtered match set
    // (same terms + filters + the caller's scope), not the truncated result
    // page and not the whole index — hence a second aggregation call with the
    // limit ignored.
    let facet_counts = state
        .search
        .facet_counts(port_query)
        .await
        .map_err(ApiError::Internal)?;
    let duration_ms = started.elapsed().as_millis() as u64;

    // Auto-track search execution (analytics.md §Auto-Emitted Events).
    // Identity is bound to the verified caller and the workspace scope was
    // validated above (task-146 review round 2) — the event reflects who
    // actually searched and a scope they may legitimately attribute to,
    // never caller-forged values. The MCP gyre_search tool records the
    // same event through the same helper (task-146 review round 3).
    super::analytics::record_search_query(
        &state,
        &auth,
        &q,
        &results,
        duration_ms,
        workspace_scope.as_ref(),
    )
    .await;

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
    auth: AuthenticatedAgent,
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
    // §Access Scoping: suggestions are scoped exactly like search results —
    // the caller never sees titles of entities outside their accessible
    // workspaces/repos.
    let scope = crate::search_scope::derive_search_scope(&state, &auth).await;
    let suggestions = state
        .search
        .suggest(
            SearchQuery::parse(q)
                .with_entity_type(None)
                .with_workspace_id(None)
                .with_limit(limit)
                .with_scope(
                    scope.tenant_id,
                    scope.accessible_workspace_ids,
                    scope.accessible_repo_ids,
                ),
        )
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
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<FacetsResponse>, ApiError> {
    // No text filter: aggregate the caller's entire accessible set — for an
    // Admin that is the whole tenant index, for everyone else their scoped
    // slice. Counts never include documents the caller cannot access
    // (§Access Scoping: counts must not leak).
    let scope = crate::search_scope::derive_search_scope(&state, &auth).await;
    let facets = state
        .search
        .facet_counts(SearchQuery::parse("").with_limit(0).with_scope(
            scope.tenant_id,
            scope.accessible_workspace_ids,
            scope.accessible_repo_ids,
        ))
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
        let app = crate::build_router(state.clone());

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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
        // Identity is bound to the verified caller (global test token →
        // the system principal), not a caller-forged value.
        assert_eq!(ev.agent_id.as_deref(), Some("system"));
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
        let app = crate::build_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=")
                    .header("Authorization", "Bearer test-token")
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

    #[tokio::test]
    async fn search_workspace_scope_validated_against_caller_tenant() {
        // Task-146 review round 2 regression: the workspace_id Query param
        // is the search.query event's workspace attribution — a forged
        // cross-tenant attribution is rejected and never lands in the
        // victim scope's filtered analytics.
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-own"),
                gyre_common::Id::new("default"),
                "Own",
                "own",
                1000,
            ))
            .await
            .unwrap();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-victim"),
                gyre_common::Id::new("tenant-other"),
                "Victim",
                "victim",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state.clone());

        // Own-tenant workspace filter: accepted and attributed.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=anything&workspace_id=ws-own")
                    .header("Authorization", "Bearer test-token")
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
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].workspace_id.as_deref(), Some("ws-own"));

        // Cross-tenant workspace: Forbidden — no forged attribution.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=anything&workspace_id=ws-victim")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Unknown workspace: NotFound, not a silent attribution.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/search?q=anything&workspace_id=ws-nope")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // The rejected searches never polluted the victim scope's filtered
        // results — the exact repro from the review finding.
        let events = state
            .analytics
            .query_filtered(&gyre_ports::AnalyticsQueryFilter {
                event_name: Some("search.query".into()),
                workspace_id: Some("ws-victim".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            events.len(),
            0,
            "forged search event must not appear in the victim workspace's filtered results"
        );
    }
}
