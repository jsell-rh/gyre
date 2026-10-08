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
    }
}

async fn call(app: &axum::Router, method: &str, uri: &str, body: Option<String>) -> axum::response::Response {
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
        .search(SearchQuery {
            query: "running".to_string(),
            entity_type: None,
            workspace_id: None,
            limit: 10,
        })
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
    abac_middleware::seed_builtin_policies(&state).await;
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
    assert_eq!(resp.status(), StatusCode::CREATED, "task create through real API failed");

    let resp = call(&app, "GET", "/api/v1/search?q=quokkas", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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

    // ── Durability: fresh state on the same file still finds the doc ───────
    let state2 = build_state(TOKEN, "http://localhost:3000", None);
    let persisted = state2
        .search
        .search(SearchQuery {
            query: "quokka".to_string(),
            entity_type: Some("task".to_string()),
            workspace_id: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(
        persisted.len(),
        1,
        "search index is not durable across build_state — mem-backed regression"
    );
    assert_eq!(persisted[0].title, "Quokka census");
}
