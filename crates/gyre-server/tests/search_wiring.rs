//! Server-level wiring test for the persistent search backend (task-201,
//! search.md §Search Index) and its access scoping (task-203, search.md
//! §Access Scoping).
//!
//! Proves the acceptance criteria the adapter tests cannot: under
//! `GYRE_DATABASE_URL=sqlite://<file>`, `build_state` wires `state.search` to
//! the SQLite FTS5 adapter via the `store!` backend-selection macro — not to
//! the in-memory `MemSearchAdapter` — and every query that crosses the HTTP
//! surface is scoped to the authenticated caller.
//!
//! This file is a separate integration-test binary (own process), so setting
//! `GYRE_DATABASE_URL` here cannot race the env of other test binaries.
//! Tests within this binary share the env var, so every phase that pokes it
//! holds `ENV_LOCK` across its `build_state` calls.
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
//! Access-scoping kill conditions (task-203, §Access Scoping):
//! - a user with an accepted membership in workspace A but not B, searching a
//!   term that matches documents in both, receives ONLY workspace-A results
//!   and the response `total` excludes workspace B;
//! - passing the other workspace's id as a `workspace_id` query param yields
//!   zero results (the param narrows, it can never widen);
//! - a repo-scoped agent receives only its repo's documents; a
//!   workspace-orchestrator agent receives all repos in its workspace;
//! - admin callers are unscoped within the tenant;
//! - suggest and facet counts enforce the same scope — a scoped caller's
//!   autocomplete titles and facet counts never include workspace B.
//!
//! task-153 endpoint legs ride the same middleware stack: GET
//! /search/suggest (prefix autocomplete, empty-prefix and auth rejection),
//! GET /search/facets (whole-index aggregation), and facet_counts on the
//! /search response (filtered-set aggregation).

use axum::body::Body;
use gyre_common::Id;
use gyre_domain::{User, UserRole, Workspace, WorkspaceMembership, WorkspaceRole};
use gyre_ports::search::{SearchDocument, SearchQuery};
use gyre_server::{abac_middleware, build_router, build_state};
use http::{Request, StatusCode};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use tower::ServiceExt;

const TOKEN: &str = "search-wiring-token";

/// Serializes env-var mutation across tests in this binary. `LazyLock`
/// initialization (not a const call in the static initializer) so the binary
/// compiles on toolchains/tokio builds where `Mutex::new` is not const.
static ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Point `GYRE_DATABASE_URL` at a temp SQLite file. Pokes the env var while
/// holding `ENV_LOCK` — safe because this integration test binary runs in its
/// own process and every env-touching phase takes the lock.
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
        tenant_id: None,
        facets: HashMap::new(),
        updated_at: None,
    }
}

fn sha256_hex(input: &str) -> String {
    let result = Sha256::digest(input.as_bytes());
    result.iter().map(|b| format!("{b:02x}")).collect()
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> axum::response::Response {
    call_with_token(app, method, uri, body, TOKEN).await
}

async fn call_with_token(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    token: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn seed_workspace(state: &gyre_server::AppState, id: &str, tenant: &str) {
    state
        .workspaces
        .create(&Workspace::new(
            Id::new(id),
            Id::new(tenant),
            format!("Workspace {id}"),
            format!("ws-{id}"),
            0,
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_url_wires_fts5_search_port() {
    let _guard = ENV_LOCK.lock().await;
    let (_dir, _url) = sqlite_url();

    // ── Port-level: FTS behavior, not substring behavior ──────────────────
    let state = build_state(TOKEN, "http://localhost:3000", None);

    // A real workspace row: index-time tenant derivation resolves against it
    // (§Access Scoping step 1 — the doc's tenant is looked up, not assumed).
    seed_workspace(&state, "ws-wiring", "default").await;
    let mut wiring_doc = doc(
        "task",
        "wiring-1",
        "Janitor rotation",
        "The janitor runs nightly maintenance sweeps.",
    );
    wiring_doc.workspace_id = Some("ws-wiring".to_string());
    state.search.index(wiring_doc).await.unwrap();

    // Unscoped-within-tenant query (Admin shape): tenant "default" matches
    // the doc whose tenant was derived from the workspace's scope row.
    // Built via `SearchQuery::parse` so the raw query and its query-language
    // parse stay consistent, then scoped.
    let admin_query = |q: &str| {
        SearchQuery::parse(q.to_string())
            .with_limit(10)
            .with_scope("default".to_string(), None, None)
    };

    // Porter stemming: "running" stems to "run" and matches indexed "runs".
    // MemSearchAdapter does lowercase `contains`: "…runs nightly…".contains("running")
    // is false — this query returns empty under a mem regression.
    let stemmed = state.search.search(admin_query("running")).await.unwrap();
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

    // Tenant boundary at the port: another tenant sees nothing.
    let other_tenant = state
        .search
        .search(SearchQuery {
            tenant_id: "other-tenant".to_string(),
            ..admin_query("running")
        })
        .await
        .unwrap();
    assert!(other_tenant.is_empty(), "cross-tenant leak at the port");

    // ── Router-level: real API surface over the same wired port ───────────
    abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app = build_router(Arc::clone(&state));

    // Real task-create callsite indexes into the same backend, scoped to the
    // task's workspace.
    let resp = call(
        &app,
        "POST",
        "/api/v1/tasks",
        Some(
            serde_json::json!({
                "title": "Quokka census",
                "description": "Count quokkas on the island every quarter.",
                "workspace_id": "ws-wiring"
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
    let body = body_json(resp).await;
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
        .search(SearchQuery {
            entity_type: Some("task".to_string()),
            ..admin_query("quokka")
        })
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
    let mut ancient = doc(
        "task",
        "ancient",
        "Ancient survey",
        "ancient relic keyword census",
    );
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
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
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
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(2), "{body}");

    // Facet filter narrows: the API-created task carries status=backlog.
    let resp = call(
        &app,
        "GET",
        "/api/v1/search?q=quokkas%20status:backlog",
        None,
    )
    .await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["results"][0]["title"], "Quokka census");
    // facet_counts aggregates the facet-filtered match set.
    assert_eq!(body["facet_counts"]["status"]["backlog"].as_u64(), Some(1));

    // Non-matching facet value: nothing, not everything.
    let resp = call(&app, "GET", "/api/v1/search?q=quokkas%20status:open", None).await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["total"].as_u64(), Some(0), "{body}");

    // Facet-only query (no text) browses the filtered set.
    let resp = call(&app, "GET", "/api/v1/search?q=status:backlog", None).await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
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
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        body["results"].as_array().map(Vec::len),
        Some(2),
        "type: facet must override the entity_type param: {body}"
    );

    // since: excludes the 40-day-old doc; absolute bounds do not.
    let resp = call(&app, "GET", "/api/v1/search?q=census%20since:7d", None).await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
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
        "since:7d must exclude the old doc: {body}"
    );

    let resp = call(
        &app,
        "GET",
        "/api/v1/search?q=census%20since:2000-01-01",
        None,
    )
    .await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["results"].as_array().map(Vec::len), Some(2), "{body}");

    let resp = call(
        &app,
        "GET",
        "/api/v1/search?q=census%20until:2000-01-01",
        None,
    )
    .await;
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["total"].as_u64(), Some(0), "{body}");
}

#[tokio::test]
async fn search_is_access_scoped_to_the_authenticated_caller() {
    let _guard = ENV_LOCK.lock().await;
    let (_dir, _url) = sqlite_url();
    let state = build_state(TOKEN, "http://localhost:3000", None);
    abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();

    // Two workspaces in the caller's tenant, each with documents matching
    // "needle"; ws-a additionally splits its docs across two repos.
    seed_workspace(&state, "ws-a", "default").await;
    seed_workspace(&state, "ws-b", "default").await;

    let mut a_repo1 = doc("task", "a-repo1", "needle", "needle in ws-a repo-1");
    a_repo1.workspace_id = Some("ws-a".to_string());
    a_repo1.repo_id = Some("repo-1".to_string());
    let mut a_repo2 = doc("task", "a-repo2", "needle", "needle in ws-a repo-2");
    a_repo2.workspace_id = Some("ws-a".to_string());
    a_repo2.repo_id = Some("repo-2".to_string());
    let mut b_doc = doc("task", "b-doc", "needle", "needle in ws-b");
    b_doc.workspace_id = Some("ws-b".to_string());
    for d in [a_repo1, a_repo2, b_doc] {
        state.search.index(d).await.unwrap();
    }

    // ── Human user: membership in ws-a only ────────────────────────────────
    let mut alice = User::new(Id::new("user-alice"), "ext-alice", "alice", 0);
    alice.roles = vec![UserRole::Developer];
    // Bind alice to the tenant the seeded workspaces live in: API-key auth
    // fail-closes (403) on users without a tenant binding (task-099 F1).
    alice.tenant_id = Some(Id::new("default"));
    state.users.create(&alice).await.unwrap();
    let alice_key = "gyre_alice_search_key";
    state
        .api_keys
        .create(&sha256_hex(alice_key), &alice.id, "search-test")
        .await
        .unwrap();
    let mut membership = WorkspaceMembership::new(
        Id::new("mem-alice-a"),
        alice.id.clone(),
        Id::new("ws-a"),
        WorkspaceRole::Developer,
        Id::new("inviter"),
        0,
    );
    membership.accepted = true;
    state
        .workspace_memberships
        .create(&membership)
        .await
        .unwrap();

    let app = build_router(Arc::clone(&state));

    // Admin (global token): unscoped within the tenant — sees every doc.
    let resp = call(&app, "GET", "/api/v1/search?q=needle", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["total"], 3, "admin must see all tenant docs: {body}");

    // Alice: membership in ws-a only — ws-b's matching doc is absent from
    // both the results and the total. If scoping were a post-filter that
    // still leaked counts, `total` would include ws-b.
    let resp = call_with_token(&app, "GET", "/api/v1/search?q=needle", None, alice_key).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(body["total"], 2, "scoped total must exclude ws-b: {body}");
    assert_eq!(results.len(), 2);
    assert!(
        results.iter().all(|r| r["entity_id"] != "b-doc"),
        "workspace-b document leaked to a workspace-a member: {body}"
    );
    // Facet counts ride the same scope: ws-b is absent from the workspace
    // facet — a count leak here is exactly what §Access Scoping forbids.
    let fc = &body["facet_counts"];
    assert_eq!(
        fc["workspace"]["ws-a"].as_u64(),
        Some(2),
        "scoped facet_counts must count only ws-a docs: {fc}"
    );
    assert!(
        fc["workspace"].get("ws-b").is_none(),
        "facet_counts leaked workspace b to a workspace-a member: {fc}"
    );

    // Suggest is scoped the same way: alice's autocomplete sees only ws-a
    // titles, never ws-b's.
    let resp = call_with_token(
        &app,
        "GET",
        "/api/v1/search/suggest?q=need",
        None,
        alice_key,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let suggestions = body["suggestions"].as_array().unwrap();
    assert_eq!(
        suggestions.len(),
        2,
        "scoped suggest must return only ws-a titles: {body}"
    );
    assert!(
        suggestions.iter().all(|s| s["entity_id"] != "b-doc"),
        "suggest leaked a workspace-b title: {body}"
    );

    // The whole-accessible-set facets endpoint is scoped too: ws-b never
    // appears in alice's workspace facet counts.
    let resp = call_with_token(&app, "GET", "/api/v1/search/facets", None, alice_key).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let facets = &body["facets"];
    assert_eq!(
        facets["workspace"]["ws-a"].as_u64(),
        Some(2),
        "facets endpoint must count only accessible docs: {facets}"
    );
    assert!(
        facets["workspace"].get("ws-b").is_none(),
        "facets endpoint leaked workspace b counts: {facets}"
    );

    // Alice requesting ws-b via the query param: zero results, not an error —
    // the param narrows within the accessible set, it can never widen.
    let resp = call_with_token(
        &app,
        "GET",
        "/api/v1/search?q=needle&workspace_id=ws-b",
        None,
        alice_key,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(
        body["total"], 0,
        "client workspace_id widened access: {body}"
    );
    assert!(body["results"].as_array().unwrap().is_empty());

    // ── Repo-scoped agent (repo orchestrator): only its repo ───────────────
    let repo_agent_jwt = state
        .agent_signing_key
        .mint_orchestrator(
            "agent-repo",
            "system",
            &state.base_url,
            3600,
            "ws-a",
            Some("repo-1"),
            "repo_orchestrator",
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", "agent-repo", repo_agent_jwt.clone())
        .await
        .unwrap();
    let resp = call_with_token(
        &app,
        "GET",
        "/api/v1/search?q=needle",
        None,
        &repo_agent_jwt,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        1,
        "repo-scoped agent must see only its repo: {body}"
    );
    assert_eq!(results[0]["entity_id"], "a-repo1");

    // Repo-scoped agent passing another workspace id: still zero widening.
    let resp = call_with_token(
        &app,
        "GET",
        "/api/v1/search?q=needle&workspace_id=ws-b",
        None,
        &repo_agent_jwt,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(
        body["total"], 0,
        "agent widened scope via workspace_id: {body}"
    );

    // ── Workspace orchestrator: all repos in its workspace, nothing outside ─
    let ws_agent_jwt = state
        .agent_signing_key
        .mint_orchestrator(
            "agent-ws",
            "system",
            &state.base_url,
            3600,
            "ws-a",
            None,
            "workspace_orchestrator",
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", "agent-ws", ws_agent_jwt.clone())
        .await
        .unwrap();
    let resp = call_with_token(&app, "GET", "/api/v1/search?q=needle", None, &ws_agent_jwt).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        2,
        "workspace orchestrator sees all its repos: {body}"
    );
    assert!(
        results.iter().all(|r| r["entity_id"] != "b-doc"),
        "workspace orchestrator saw a document outside its workspace: {body}"
    );

    // ── Worker agent without orchestrator claims: its agent record's scope ─
    let mut worker = gyre_domain::Agent::new(Id::new("agent-worker"), "worker", 0);
    worker.workspace_id = Id::new("ws-a");
    state.agents.create(&worker).await.unwrap();
    let worker_jwt = state
        .agent_signing_key
        .mint("agent-worker", "task-9", "system", &state.base_url, 3600)
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", "agent-worker", worker_jwt.clone())
        .await
        .unwrap();
    // Worker's agent record scopes it to ws-a: both ws-a docs, never ws-b.
    let resp = call_with_token(&app, "GET", "/api/v1/search?q=needle", None, &worker_jwt).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        2,
        "worker agent must see its workspace: {body}"
    );
    assert!(
        results.iter().all(|r| r["entity_id"] != "b-doc"),
        "worker agent saw a document outside its workspace: {body}"
    );

    // A worker agent with NO agent record: fail closed — zero results.
    let ghost_jwt = state
        .agent_signing_key
        .mint("agent-ghost", "task-9", "system", &state.base_url, 3600)
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", "agent-ghost", ghost_jwt.clone())
        .await
        .unwrap();
    let resp = call_with_token(&app, "GET", "/api/v1/search?q=needle", None, &ghost_jwt).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(
        body["total"], 0,
        "unknown worker agent must fail closed, not open: {body}"
    );
}
