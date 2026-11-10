//! Server-level wiring test for the persistent search backend (task-201,
//! search.md §Search Index).
//!
//! Proves the acceptance criterion the adapter tests cannot: under
//! `GYRE_DATABASE_URL=sqlite://<file>`, `build_state` wires `state.search` to
//! the SQLite FTS5 adapter via the `store!` backend-selection macro — not to
//! the in-memory `MemSearchAdapter`.
//!
//! This file is a separate integration-test binary (own process), so setting
//! `GYRE_DATABASE_URL` here cannot race the env of other test binaries.
//!
//! The HTTP leg drives the full router middleware stack (require_auth →
//! last_seen → ABAC → rate limit → CatchPanic → handler) via
//! `tower::ServiceExt::oneshot` rather than a bound TCP listener: same
//! request path, and it stays valid in sandboxed environments where loopback
//! serving is unavailable.
//!
//! Regression kill conditions (each fails if the wiring reverts to mem):
//! - port-level: porter stemming ("running" matches "runs") — substring
//!   matching cannot do this;
//! - router-level: the real POST /api/v1/tasks → GET /api/v1/search flow
//!   returns FTS5 snippet markers and a bm25-derived score;
//! - durability: a second `build_state` on the same file still finds the doc
//!   (an in-memory index loses it).
//!
//! task-153 endpoint legs ride the same middleware stack: GET
//! /search/suggest (prefix autocomplete, empty-prefix and auth rejection),
//! GET /search/facets (whole-index aggregation), and facet_counts on the
//! /search response (filtered-set aggregation).

use axum::body::Body;
use gyre_ports::search::{SearchDocument, SearchQuery};
use gyre_server::{abac_middleware, build_router, build_state};
use http::{Request, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "search-wiring-token";

/// Point `GYRE_DATABASE_URL` at a temp SQLite file. Pokes the env var once at
/// startup — safe because this integration test binary runs in its own process.
fn sqlite_url() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gyre-search-test.db");
    let url = format!("sqlite://{}", path.to_str().unwrap());
    unsafe { std::env::set_var("GYRE_DATABASE_URL", &url) };
    (dir, url)
}

fn doc(entity_type: &str, entity_id: &str, title: &str, body: &str) -> SearchDocument {
    SearchDocument {
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        workspace_id: None,
        repo_id: None,
        facets: HashMap::new(),
        updated_at: None,
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn sqlite_url_wires_fts5_search_port() {
    let (_dir, _url) = sqlite_url();

    // ── Port-level: FTS behavior, not substring behavior ──────────────────
    let state = build_state(TOKEN, "http://localhost:3000", None);

    state
        .search
        .index(doc(
            "task",
            "wiring-1",
            "Janitor rotation",
            "The janitor runs nightly maintenance sweeps.",
        ))
        .await
        .unwrap();

    // Porter stemming: "running" stems to "run" and matches indexed "runs".
    // MemSearchAdapter does lowercase `contains`: "…runs nightly…".contains("running")
    // is false — this query returns empty under a mem regression.
    let stemmed = state
        .search
        .search(SearchQuery::parse("running".to_string()).with_entity_type(None).with_workspace_id(None).with_limit(10))
        .await
        .unwrap();
    assert_eq!(
        stemmed.len(),
        1,
        "porter stemming must match runs~running; empty/mem-backed result means \
         state.search is NOT the FTS5 adapter"
    );
    assert_eq!(stemmed[0].entity_id, "wiring-1");
    // FTS5 snippet markers around the match — MemSearchAdapter emits none.
    assert!(
        stemmed[0].snippet.contains("**"),
        "snippet lacks FTS5 match markers: {}",
        stemmed[0].snippet
    );

    // ── Router-level: real API surface over the same wired port ───────────
    abac_middleware::seed_builtin_policies(&state)
        .await
        .expect("seed built-in policies");
    let app = build_router(Arc::clone(&state));

    // Real task-create callsite indexes into the same backend.
    let resp = call(
        &app,
        "POST",
        "/api/v1/tasks",
        Some(
            serde_json::json!({
                "title": "Quokka census",
                "description": "Count quokkas on the island every quarter."
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "task create through real API failed"
    );

    let resp = call(&app, "GET", "/api/v1/search?q=quokkas", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let results = body["results"].as_array().unwrap();
    // "quokkas" porter-stems to "quokka" and matches the indexed doc.
    assert_eq!(
        results.len(),
        1,
        "GET /api/v1/search found no stemmed match: {body}"
    );
    let hit = &results[0];
    assert_eq!(hit["entity_type"], "task");
    assert_eq!(hit["title"], "Quokka census");
    let snippet = hit["snippet"].as_str().unwrap();
    // FTS5 snippet() marks the raw query token with ** ** — assert the marker
    // pair around the echoed token (either casing) to prove FTS5 highlighting.
    assert!(
        snippet.contains("**quokkas**") || snippet.contains("**Quokkas**"),
        "HTTP search snippet lacks FTS5 match markers: {snippet}"
    );
    assert!(
        hit["score"].as_f64().unwrap() > 0.0,
        "bm25-derived score must be positive: {hit}"
    );
    // Facet round-trip through the API surface.
    assert_eq!(hit["facets"]["status"], "backlog");

    // ── §API/§Response Format: facet_counts on the search response ───────
    // Aggregated over the filtered match set (the quokka task), not the
    // whole index and not a truncated page.
    let fc = &body["facet_counts"];
    assert_eq!(
        fc["type"]["task"].as_u64(),
        Some(1),
        "facet_counts.type must count the matching task: {fc}"
    );
    assert_eq!(
        fc["status"]["backlog"].as_u64(),
        Some(1),
        "facet_counts.status must aggregate the match set's facets: {fc}"
    );

    // ── §API: GET /search/suggest — prefix autocomplete ──────────────────
    let resp = call(&app, "GET", "/api/v1/search/suggest?q=quok", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let suggestions = body["suggestions"].as_array().unwrap();
    assert_eq!(
        suggestions.len(),
        1,
        "suggest must prefix-match 'quok' against 'Quokka census': {body}"
    );
    assert_eq!(suggestions[0]["text"], "Quokka census");
    assert_eq!(suggestions[0]["entity_type"], "task");
    assert!(!suggestions[0]["entity_id"].as_str().unwrap().is_empty());

    // Empty prefix: empty list — never "every title in the index".
    let resp = call(&app, "GET", "/api/v1/search/suggest?q=", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["suggestions"].as_array().map(Vec::len),
        Some(0),
        "empty prefix must yield no suggestions: {body}"
    );

    // Non-matching prefix: empty list.
    let resp = call(&app, "GET", "/api/v1/search/suggest?q=zzz", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["suggestions"].as_array().map(Vec::len), Some(0));

    // Suggest is authenticated like /search itself: no token → 401.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/search/suggest?q=quok")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "suggest must sit behind require_auth"
    );
    // Facets endpoint likewise.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/search/facets")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "facets must sit behind require_auth"
    );

    // ── §API: GET /search/facets — whole-index aggregation ───────────────
    let resp = call(&app, "GET", "/api/v1/search/facets", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let facets = &body["facets"];
    // wiring-1 (indexed directly) + the API-created quokka task.
    assert_eq!(
        facets["type"]["task"].as_u64(),
        Some(2),
        "facets must aggregate the whole index: {facets}"
    );
    // Only the API-created task carries a status facet (backlog).
    assert_eq!(facets["status"]["backlog"].as_u64(), Some(1));

    // ── Durability: fresh state on the same file still finds the doc ───────
    let state2 = build_state(TOKEN, "http://localhost:3000", None);
    let persisted = state2
        .search
        .search(SearchQuery::parse("quokka".to_string()).with_entity_type(Some("task".to_string())).with_workspace_id(None).with_limit(10))
        .await
        .unwrap();
    assert_eq!(
        persisted.len(),
        1,
        "search index is not durable across build_state — mem-backed regression"
    );
    assert_eq!(persisted[0].title, "Quokka census");

    // ── §Query Language (task-154): quoted phrases, facets, time bounds ────
    // A doc with both words but never adjacent: a phrase query must not match.
    state
        .search
        .index(doc(
            "task",
            "scattered",
            "Quokka elsewhere",
            "Count only the island quokkas every quarter.",
        ))
        .await
        .unwrap();
    // A 40-day-old doc: since: must exclude it, absolute bounds must not.
    let mut ancient = doc("task", "ancient", "Ancient survey", "ancient relic keyword census");
    ancient.updated_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 40 * 86_400,
    );
    state.search.index(ancient).await.unwrap();

    // Quoted phrase: exact adjacency ("count quokkas" porter-stems to the
    // adjacent token pair count~quokka in the census task's body only).
    let resp = call(&app, "GET", "/api/v1/search?q=%22count%20quokkas%22", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let titles: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Quokka census"],
        "phrase query must require adjacency, not both words: {body}"
    );

    // Unquoted, the same words are independent terms — both quokka docs match.
    let resp = call(&app, "GET", "/api/v1/search?q=count%20quokkas", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(2), "{body}");

    // Facet filter narrows: the API-created task carries status=backlog.
    let resp = call(&app, "GET", "/api/v1/search?q=quokkas%20status:backlog", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["results"][0]["title"], "Quokka census");
    // facet_counts aggregates the facet-filtered match set.
    assert_eq!(body["facet_counts"]["status"]["backlog"].as_u64(), Some(1));

    // Non-matching facet value: nothing, not everything.
    let resp = call(&app, "GET", "/api/v1/search?q=quokkas%20status:open", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["total"].as_u64(), Some(0), "{body}");

    // Facet-only query (no text) browses the filtered set.
    let resp = call(&app, "GET", "/api/v1/search?q=status:backlog", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["results"][0]["title"], "Quokka census");

    // Query-string facet takes precedence over the entity_type param
    // (the param is a fallback shortcut, §Query Language).
    let resp = call(
        &app,
        "GET",
        "/api/v1/search?q=quokkas%20type:task&entity_type=mr",
        None,
    )
    .await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(
        body["results"].as_array().map(Vec::len),
        Some(2),
        "type: facet must override the entity_type param: {body}"
    );

    // since: excludes the 40-day-old doc; absolute bounds do not.
    let resp = call(&app, "GET", "/api/v1/search?q=census%20since:7d", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let titles: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Quokka census"], "since:7d must exclude the old doc: {body}");

    let resp = call(&app, "GET", "/api/v1/search?q=census%20since:2000-01-01", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(2), "{body}");

    let resp = call(&app, "GET", "/api/v1/search?q=census%20until:2000-01-01", None).await;
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["total"].as_u64(), Some(0), "{body}");
}
