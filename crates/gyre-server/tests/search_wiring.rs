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
//! Regression kill conditions (each fails if the wiring reverts to mem):
//! - port-level: porter stemming ("running" matches "runs") — substring
//!   matching cannot do this;
//! - server-level: real HTTP `GET /api/v1/search` through the task-create
//!   callsite returns FTS5 snippet markers and bm25 scores;
//! - durability: a second `build_state` on the same file still finds the doc
//!   (an in-memory index loses it).

use gyre_ports::search::{SearchDocument, SearchQuery};
use gyre_server::{abac_middleware, build_router, build_state, AppState};
use std::collections::HashMap;
use std::sync::Arc;

const TOKEN: &str = "search-wiring-token";

/// Read `GYRE_DATABASE_URL`, pointing it at a temp SQLite file.
/// Pokes the env var once at startup — safe because this integration test
/// binary runs in its own process.
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

async fn spawn_server(state: Arc<AppState>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = build_router(Arc::clone(&state));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
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

    // ── Server-level: the /api/v1/search route over the same wired port ────
    abac_middleware::seed_builtin_policies(&state).await;
    let base = spawn_server(Arc::clone(&state)).await;
    let client = reqwest::Client::new();

    // Real task-create callsite indexes into the same backend.
    let resp = client
        .post(format!("{base}/api/v1/tasks"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "title": "Quokka census",
            "description": "Count quokkas on the island every quarter."
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "task create through real API failed");

    let resp = client
        .get(format!("{base}/api/v1/search?q=quokkas"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
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
    assert!(
        hit["snippet"]
            .as_str()
            .unwrap()
            .contains("**Quokka**") || hit["snippet"].as_str().unwrap().contains("**quokka**"),
        "HTTP search snippet lacks FTS5 markers: {hit}"
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
