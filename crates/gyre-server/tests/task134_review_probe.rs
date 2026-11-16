//! Task-134 review probe: verify the scoped-token enforcement behaviors that
//! the e2e tests cannot cover in a no-loopback sandbox, using oneshot router
//! calls (no listener required). Scope capability strings are minted exactly
//! as the gate executor mints them (`review:submit` for reviewers,
//! `validation:report` for validators).

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use tower::ServiceExt; // oneshot

async fn test_state() -> std::sync::Arc<gyre_server::AppState> {
    gyre_server::build_state("gyre-test-token", "http://localhost:0", None)
}

/// Ensure a Workspace row backs the probe MR's default workspace scope and
/// return its tenant id, so §1 mint_scoped callers resolve the tenant the
/// way production does (MR → workspace → tenant), never a fabricated
/// literal. Idempotent.
async fn ensure_probe_workspace(
    state: &std::sync::Arc<gyre_server::AppState>,
    ws_id: &str,
    tenant_id: &str,
) -> String {
    let ws_id = gyre_common::Id::new(ws_id);
    if let Ok(Some(ws)) = state.workspaces.find_by_id(&ws_id).await {
        return ws.tenant_id.to_string();
    }
    let ws = gyre_domain::Workspace::new(
        ws_id.clone(),
        gyre_common::Id::new(tenant_id),
        "probe workspace",
        ws_id.as_str().to_string(),
        1,
    );
    state.workspaces.create(&ws).await.unwrap();
    tenant_id.to_string()
}

/// Scoped `review:submit` JWT must be bound to its own subject when
/// submitting a review (forged reviewer_agent_id ignored).
#[tokio::test]
async fn scoped_review_token_reviewer_identity_is_bound() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    // Seed an MR.
    let mr_id = gyre_common::Id::new("mr-probe-1");
    let mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        gyre_common::Id::new("repo-probe"),
        gyre_common::Id::new("ws-probe"),
        "t",
        "feature",
        "main",
        1,
    );
    state.merge_requests.create(&mr).await.unwrap();

    // Mint a scoped token for a gate agent. Tenant comes from the MR's
    // workspace ("default", the documented single-tenant scope — a real
    // Workspace row is seeded so the §1 mint resolves it like production).
    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let gate_agent_id = "gate-review-probe".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "review:submit",
            &tenant_id,
            Some("test-reviewer"),
        )
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

    let st = resp.status();
    let bd = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let tx = String::from_utf8_lossy(&bd);
    println!("SUBMIT-RESPONSE status={st} body={tx}");
    assert_eq!(st, StatusCode::CREATED, "body: {tx}");
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
    // Seed builtin policies so the agent role WOULD be permitted on this
    // route (builtin-agent-scoped-access allows Agent-role reads) — that
    // way the 403 below is attributable to the scoped-token allow-list,
    // not to an empty policy store's default-deny.
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let gate_agent_id = "gate-review-probe-2".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "review:submit",
            &tenant_id,
            Some("test-reviewer"),
        )
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

/// Task-195 review repair: a `review:submit`-scoped gate-agent token must
/// NOT reach the REST agent-inbox route (`GET /api/v1/agents/{id}/messages`).
/// The route is kept off the reviewer allow-list as least privilege /
/// defense in depth: a single-purpose review token's ReviewRequest is
/// delivered via its launch env and the /mcp surface
/// (`gyre_message_poll` derives the inbox from the verified token
/// subject — inherently own-inbox), so it has no need for the REST
/// inbox route. (The handler actually routed at that path,
/// `messages::poll_messages`, does enforce own-inbox for non-admin
/// callers; the "no caller-identity check, drain-and-clear" handler
/// from earlier review rounds was the unrouted legacy `agent_messages`
/// module, since deleted.) Discriminator: builtin policies are seeded
/// so the agent role WOULD permit the read; the 403 is attributable
/// solely to the scoped-token allow-list, and re-adding the route there
/// flips this test to a 200.
#[tokio::test]
async fn scoped_review_token_cannot_read_agent_inbox_route() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    // A distinct victim agent whose inbox the scoped token must not touch.
    let victim = gyre_domain::Agent::new(
        gyre_common::Id::new("agent-inbox-victim-1"),
        "inbox-victim",
        0,
    );
    state.agents.create(&victim).await.unwrap();

    let gate_agent_id = "gate-review-probe-inbox".to_string();
    // §1 Token Scoping: the mint needs the tenant of a real workspace row
    // and the gate persona slug, like every production gate path.
    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "review:submit",
            &tenant_id,
            Some("test-reviewer"),
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    // GET /api/v1/agents/{victim}/messages with the scoped token: outside
    // the review allow-list (and deliberately kept off it), so denied even
    // though the builtin agent-role policy would permit the read.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/agents/agent-inbox-victim-1/messages")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "review-scoped gate-agent token must not read another agent's inbox \
         (route is off the reviewer allow-list: least privilege)"
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
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "review:submit",
            ws.tenant_id.as_str(),
            Some("test-reviewer"),
        )
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
        text.contains("gate-agent scoped tokens are read-only"),
        "expected scoped-token denial message, got: {text}"
    );
}

/// A revoked scoped token (kv_remove) must no longer authenticate.
#[tokio::test]
async fn revoked_scoped_token_is_rejected() {
    let state = test_state().await;
    let app: Router = gyre_server::build_router(state.clone());

    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let gate_agent_id = "gate-review-probe-4".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "review:submit",
            &tenant_id,
            Some("test-reviewer"),
        )
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

    // /api/v1/version is intentionally public (require_auth_middleware
    // short-circuits it), so it can never observe revocation. Use an
    // authenticated route inside the review allow-list instead: after
    // kv_remove, the auth middleware must reject the token with 401.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/merge-requests/some-mr")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "revoked scoped token must fail auth"
    );
}

/// Review F2: an AgentValidation gate agent (scope `validation:report`) must
/// NOT be able to submit a review — its contract is exit-code pass/fail, so
/// it must not influence review-based approval surfaces. The ABAC
/// allow-list denies the review route for validation-scoped tokens.
#[tokio::test]
async fn validation_scoped_token_cannot_submit_review() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    // Seed an MR.
    let mr_id = gyre_common::Id::new("mr-probe-validation");
    let mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        gyre_common::Id::new("repo-probe"),
        gyre_common::Id::new("ws-probe"),
        "t",
        "feature",
        "main",
        1,
    );
    state.merge_requests.create(&mr).await.unwrap();

    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let gate_agent_id = "gate-validate-probe".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "validation:report",
            &tenant_id,
            Some("test-reviewer"),
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    let body = serde_json::json!({
        "reviewer_agent_id": gate_agent_id,
        "decision": "approved",
        "body": "validator should not be able to do this"
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merge-requests/mr-probe-validation/reviews")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "validation-scoped token must not submit reviews: {text}"
    );
    // And no review was persisted.
    let reviews = state.reviews.list_reviews(&mr_id).await.unwrap();
    assert!(
        reviews.is_empty(),
        "validation agent must not create reviews: {reviews:?}"
    );
}

/// Review F2: a validation agent may still read the MR context it needs for
/// its domain-specific check (MR detail + diff, read-only).
#[tokio::test]
async fn validation_scoped_token_can_read_mr_context() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    let mr_id = gyre_common::Id::new("mr-probe-validation-read");
    let mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        gyre_common::Id::new("repo-probe"),
        gyre_common::Id::new("ws-probe"),
        "t",
        "feature",
        "main",
        1,
    );
    state.merge_requests.create(&mr).await.unwrap();

    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;
    let gate_agent_id = "gate-validate-probe-read".to_string();
    let token = state
        .agent_signing_key
        .mint_scoped(
            &gate_agent_id,
            "gate-1",
            "forge",
            "http://localhost:0",
            60,
            "validation:report",
            &tenant_id,
            Some("test-reviewer"),
        )
        .unwrap();
    state
        .kv_store
        .kv_set("agent_tokens", &gate_agent_id, token.clone())
        .await
        .unwrap();

    for uri in [
        format!("/api/v1/merge-requests/{}", mr_id.as_str()),
        format!("/api/v1/merge-requests/{}/diff", mr_id.as_str()),
    ] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "validation-scoped token must read MR context routes"
        );
    }
}

/// Review F3: capability comparison must be exact-match. A lookalike scope
/// (`review:submit:admin`, `xreview:submit`) is not a known gate-agent
/// capability, so it must not inherit the scoped-token route allow-list.
/// Discriminators:
/// 1. Route allow-list: the lookalike token authenticates as a normal
///    agent, so GET /api/v1/repos (outside the review allow-list) is
///    evaluated under the builtin agent-role policy and permitted — a
///    substring scope match would lock the token to the review routes and
///    deny it (403) there.
/// 2. Reviewer identity binding: since the manifest-gate review repair,
///    binding applies to EVERY agent-token principal regardless of scope,
///    so the lookalike token's review is stored under its own verified
///    token subject, never the request-body reviewer id.
#[tokio::test]
async fn lookalike_scope_does_not_inherit_review_capability() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    let mr_id = gyre_common::Id::new("mr-probe-lookalike");
    let mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        gyre_common::Id::new("repo-probe"),
        gyre_common::Id::new("ws-probe"),
        "t",
        "feature",
        "main",
        1,
    );
    state.merge_requests.create(&mr).await.unwrap();
    let tenant_id = ensure_probe_workspace(&state, "default", "tenant-probe").await;

    for lookalike in ["review:submit:admin", "xreview:submit"] {
        let agent_id = format!("gate-lookalike-{}", lookalike.replace(':', "-"));
        let token = state
            .agent_signing_key
            .mint_scoped(
                &agent_id,
                "gate-1",
                "forge",
                "http://localhost:0",
                60,
                lookalike,
                &tenant_id,
                None,
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", &agent_id, token.clone())
            .await
            .unwrap();

        // Discriminator 1 — route allow-list: the lookalike scope is not a
        // known gate-agent capability, so the scoped-token allow-list does
        // not apply and GET /api/v1/repos is evaluated under the agent-role
        // builtin policy (which permits the read). A substring scope
        // comparison would treat the lookalike as `review:submit` and deny
        // the request here.
        let resp = app
            .clone()
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
            StatusCode::OK,
            "lookalike scope '{lookalike}' must not inherit the scoped-token route allow-list"
        );

        // Discriminator 2 — reviewer identity binding: the review POST is
        // permitted by the agent-role policy, but the review is stored
        // under the token's own subject (agent-token principals are bound
        // to their verified identity regardless of scope).
        let body = serde_json::json!({
            "reviewer_agent_id": "some-other-reviewer",
            "decision": "approved",
            "body": format!("lookalike probe {lookalike}")
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/mr-probe-lookalike/reviews")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(status, StatusCode::CREATED, "body: {text}");

        let reviews = state.reviews.list_reviews(&mr_id).await.unwrap();
        let stored = reviews
            .iter()
            .find(|r| r.body.as_deref() == Some(&format!("lookalike probe {lookalike}")))
            .unwrap_or_else(|| panic!("lookalike scope request failed: {status} {text}"));
        assert_eq!(
            stored.reviewer_agent_id, agent_id,
            "an agent-token caller's review must be recorded under its own verified \
             identity, never the request-body reviewer id (scope '{lookalike}')"
        );
    }
}

/// task-139 review finding 1: the gate executor hands AgentReview /
/// AgentValidation gate agents `GYRE_SPEC_URL =
/// {base}/api/v1/merge-requests/{id}/spec` and only their scoped token
/// (`review:submit` / `validation:report`). The scoped-token allow-list
/// must include the MR spec route, or the documented spec-alignment
/// protocol (docs/agent-protocol.md) is unfetchable by the very agents
/// the URL is minted for (403 before policy evaluation).
///
/// Proves, through the full router (auth + ABAC layers, oneshot — no
/// listener required):
/// 1. a `review:submit` gate agent GETs the pinned spec: 200 + pinned
///    content (not HEAD);
/// 2. the same token is still 403 on routes outside its allow-list;
/// 3. a `validation:report` gate agent can also fetch the spec (its
///    check compares behavior against the pinned spec version).
#[tokio::test]
async fn scoped_gate_tokens_can_fetch_pinned_mr_spec() {
    let state = test_state().await;
    gyre_server::abac_middleware::seed_builtin_policies(&state)
        .await
        .unwrap();
    let app: Router = gyre_server::build_router(state.clone());

    // Real repo on disk: spec v1 committed (blob pinned), then v2 at HEAD.
    let tmp = tempfile::TempDir::new().unwrap();
    let repo_path = tmp.path().join("spec-probe-repo");
    std::fs::create_dir_all(repo_path.join("specs/system")).unwrap();
    std::fs::write(
        repo_path.join("specs/system/identity.md"),
        "# Identity v1\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["add", "."]);
    git(&["commit", "-m", "spec v1"]);
    let v1_blob = git(&["rev-parse", "HEAD:specs/system/identity.md"]);
    std::fs::write(
        repo_path.join("specs/system/identity.md"),
        "# Identity v2 — EDITED AT HEAD\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "spec v2"]);

    let ws_id = gyre_common::Id::new("ws-spec-probe");
    let ws = gyre_domain::Workspace::new(
        ws_id.clone(),
        gyre_common::Id::new("tenant-spec-probe"),
        "ws-spec-probe",
        "ws-spec-probe",
        1,
    );
    state.workspaces.create(&ws).await.unwrap();
    let repo_id = gyre_common::Id::new("repo-spec-probe");
    let repo = gyre_domain::Repository::new(
        repo_id.clone(),
        ws_id.clone(),
        "spec-probe-repo",
        repo_path.to_str().unwrap(),
        1,
    );
    state.repos.create(&repo).await.unwrap();

    let mr_id = gyre_common::Id::new("mr-spec-probe");
    let mut mr = gyre_domain::MergeRequest::new(
        mr_id.clone(),
        repo_id.clone(),
        ws_id.clone(),
        "Spec probe MR",
        "feat/spec-probe",
        "main",
        1,
    );
    // Production shape: ledger path + pinned blob sha.
    mr.spec_ref = Some(format!("system/identity.md@{v1_blob}"));
    state.merge_requests.create(&mr).await.unwrap();

    let spec_uri = format!("/api/v1/merge-requests/{}/spec", mr_id.as_str());

    for (agent, scope) in [
        ("gate-review-spec-probe", "review:submit"),
        ("gate-validate-spec-probe", "validation:report"),
    ] {
        let token = state
            .agent_signing_key
            .mint_scoped(
                agent,
                "gate-1",
                "forge",
                "http://localhost:0",
                60,
                scope,
                // §1: tenant resolved from the MR's seeded workspace row, the
                // same fail-closed chain the gate executor uses in production.
                ws.tenant_id.as_str(),
                Some("gate-spec-probe"),
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", agent, token.clone())
            .await
            .unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&spec_uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status,
            StatusCode::OK,
            "{scope} gate agent must fetch GYRE_SPEC_URL: {text}"
        );
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            json["content"], "# Identity v1\n",
            "{scope} sees pinned spec"
        );
        assert_eq!(json["spec_sha"], v1_blob.as_str());

        // Same token, route outside the allow-list: still contained.
        let resp = app
            .clone()
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
            "{scope} gate agent must stay denied outside its allow-list"
        );

        // Teardown: revoke so later iterations mint fresh tokens.
        state
            .kv_store
            .kv_remove("agent_tokens", agent)
            .await
            .unwrap();
    }
}
