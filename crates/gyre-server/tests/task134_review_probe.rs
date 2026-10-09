//! Task-134 review probe: verify the scoped-token enforcement behaviors that
//! the e2e tests cannot cover in a no-loopback sandbox, using oneshot router
//! calls (no listener required).

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use tower::ServiceExt; // oneshot

async fn test_state() -> std::sync::Arc<gyre_server::AppState> {
    gyre_server::build_state("gyre-test-token", "http://localhost:0", None)
}

/// Scoped `review:submit` JWT must be bound to its own subject when
/// submitting a review (forged reviewer_agent_id ignored).
#[tokio::test]
async fn scoped_review_token_reviewer_identity_is_bound() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state).await;
    let app: Router = gyre_server::build_router(state.clone());

    // Seed an MR.
    let mr_id = gyre_common::Id::new("mr-probe-1");
    let mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        gyre_common::Id::new("repo-probe"),
        "t",
        "feature",
        "main",
        1,
    );
    state.merge_requests.create(&mr).await.unwrap();

    // Mint a scoped token for a gate agent.
    let gate_agent_id = "gate-review-probe".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(&gate_agent_id, "gate-1", "forge", "http://localhost:0", 60, "review:submit")
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    // Submit a review with a forged reviewer id.
    let body = serde_json::json!({
        "reviewer_agent_id": "forged-reviewer",
        "decision": "approved",
        "body": "probe"
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merge-requests/mr-probe-1/reviews")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let st = resp.status(); let bd = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap(); let tx = String::from_utf8_lossy(&bd); println!("SUBMIT-RESPONSE status={st} body={tx}"); assert_eq!(st, StatusCode::CREATED, "body: {tx}");
    let reviews = state.reviews.list_reviews(&mr_id).await.unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(
        reviews[0].reviewer_agent_id, gate_agent_id,
        "reviewer identity must be bound to the scoped token subject, not the forged request body value"
    );
}

/// A scoped `review:submit` token must be denied on routes outside the
/// allow-list (e.g. listing repos), even though the agent role would
/// otherwise allow read.
#[tokio::test]
async fn scoped_review_token_denied_outside_allowlist() {
    let state = test_state().await;
    let app: Router = gyre_server::build_router(state.clone());

    let gate_agent_id = "gate-review-probe-2".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(&gate_agent_id, "gate-1", "forge", "http://localhost:0", 60, "review:submit")
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    // GET /api/v1/repos — outside the review allow-list; agent role would
    // otherwise be permitted by builtin agent-scoped-access policy.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/repos")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "scoped review token must be denied outside the review-route allow-list"
    );
}

/// Git push with a scoped review token must be rejected read-only.
#[tokio::test]
async fn scoped_review_token_cannot_push() {
    let state = test_state().await;
    let app: Router = gyre_server::build_router(state.clone());

    // Need a repo to push to; create a workspace + repo via API or directly.
    let ws_id = gyre_common::Id::new("ws-probe");
    let ws = gyre_domain::Workspace::new(
        ws_id.clone(),
        gyre_common::Id::new("tenant-probe"),
        "ws-probe",
        "ws-probe",
        1,
    );
    state.workspaces.create(&ws).await.unwrap();
    let repo_id = gyre_common::Id::new("repo-probe-push");
    let repo = gyre_domain::Repository::new(
        repo_id.clone(),
        ws_id.clone(),
        "probe-repo",
        "/tmp/nonexistent-probe.git",
        1,
    );
    state.repos.create(&repo).await.unwrap();

    let gate_agent_id = "gate-review-probe-3".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(&gate_agent_id, "gate-1", "forge", "http://localhost:0", 60, "review:submit")
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    // git-receive-pack (push). The repo path doesn't exist so we only need
    // the 403 before any git activity — the scope check runs before
    // body processing.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/git/ws-probe/probe-repo/git-receive-pack")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/x-git-receive-pack-request")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "scoped review token push must be 403, got {status}: {text}"
    );
    assert!(
        text.contains("review-scoped"),
        "expected review-scoped denial message, got: {text}"
    );
}

/// A revoked scoped token (kv_remove) must no longer authenticate.
#[tokio::test]
async fn revoked_scoped_token_is_rejected() {
    let state = test_state().await;
    let app: Router = gyre_server::build_router(state.clone());

    let gate_agent_id = "gate-review-probe-4".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(&gate_agent_id, "gate-1", "forge", "http://localhost:0", 60, "review:submit")
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();
    // Teardown path.
    state
        .kv_store
        .kv_remove("agent_tokens", &gate_agent_id)
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/version")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "revoked scoped token must fail auth");
}
