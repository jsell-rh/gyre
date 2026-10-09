use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_ports::JjChange;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::commit_signatures::{self, CommitSignature};
use crate::AppState;

use super::error::ApiError;

#[derive(Serialize)]
pub struct JjChangeResponse {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: u64,
    pub bookmarks: Vec<String>,
}

impl From<JjChange> for JjChangeResponse {
    fn from(c: JjChange) -> Self {
        Self {
            change_id: c.change_id,
            commit_id: c.commit_id,
            description: c.description,
            author: c.author,
            timestamp: c.timestamp,
            bookmarks: c.bookmarks,
        }
    }
}

#[derive(Deserialize)]
pub struct NewChangeRequest {
    pub description: String,
}

#[derive(Deserialize)]
pub struct BookmarkRequest {
    pub name: String,
    pub change_id: String,
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub limit: Option<usize>,
}

async fn repo_path(state: &AppState, repo_id: &str) -> Result<String, ApiError> {
    let repo = state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    Ok(repo.path)
}

/// POST /api/v1/repos/:id/jj/init
pub async fn jj_init(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    state
        .jj_ops
        .jj_init(&path)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/repos/:id/jj/log
pub async fn jj_log(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Result<Json<Vec<JjChangeResponse>>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    let limit = q.limit.unwrap_or(20);
    let changes = state
        .jj_ops
        .jj_log(&path, limit)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(changes.into_iter().map(Into::into).collect()))
}

/// POST /api/v1/repos/:id/jj/new
pub async fn jj_new(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<NewChangeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;
    let change_id = state
        .jj_ops
        .jj_new(&path, &req.description)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(serde_json::json!({ "change_id": change_id })))
}

/// POST /api/v1/repos/:id/jj/squash
///
/// Squashes the working copy into its parent change and signs the resulting
/// commit SHA. Signing mode comes from `GYRE_SIGNING_MODE`:
/// - `fulcio`: keyless Sigstore signing — Fulcio certificate issued against
///   the caller's OIDC JWT, commit signed with the ephemeral key, signature
///   recorded in Rekor (task-107).
/// - `none`: no signature record.
/// - `local` (default): forge Ed25519 key (M13.8).
///
/// Any failure in the external Fulcio/Rekor stack falls back to local
/// signing with a warning — squash itself never fails because of the
/// external signing stack.
pub async fn jj_squash(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<CommitSignature>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;
    let commit_sha = state
        .jj_ops
        .jj_squash(&path)
        .await
        .map_err(ApiError::Internal)?;

    let config = &state.signing_config;
    if config.mode == commit_signatures::SigningMode::None {
        // `none` mode: skip signing entirely — no record is produced. The
        // response reports the commit with an empty signature so callers can
        // distinguish "unsigned" from "signed" without a second lookup.
        return Ok(Json(CommitSignature {
            repo_id: repo_id.clone(),
            commit_sha,
            signer_id: auth.agent_id.clone(),
            task_id: String::new(),
            spawned_by: String::new(),
            algorithm: String::new(),
            signature: String::new(),
            signing_key_id: String::new(),
            signed_at: 0,
            sigstore_mode: gyre_ports::SigstoreMode::Local,
            oidc_subject: String::new(),
            oidc_issuer: String::new(),
            certificate_pem: None,
            certificate_chain_pem: None,
            rekor_entry_id: None,
        }));
    }

    // Attribution comes from the caller's validated JWT claims — never
    // placeholder literals (task-107 F2). Non-JWT callers (dev token, API
    // key) fall back to the resolved agent id with empty task/user fields.
    let attribution = commit_signatures::SigningAttribution::from_auth(&auth);

    let record = if config.mode == commit_signatures::SigningMode::Fulcio {
        // Keyless: present the caller's JWT to Fulcio. The token was
        // validated at authentication time; Fulcio re-validates the OIDC
        // identity itself before issuing.
        let jwt = auth.bearer_token.clone().unwrap_or_default();
        let transport = crate::sigstore::production_transport(state.http_client.clone());
        match crate::sigstore::sign_commit_keyless(
            &repo_id,
            &commit_sha,
            &attribution,
            &jwt,
            config,
            transport.as_ref(),
        )
        .await
        {
            Ok(signed) => {
                let record = signed.record;
                tracing::info!(
                    commit_sha = %commit_sha,
                    rekor_entry_id = ?record.rekor_entry_id,
                    "jj squash: commit signed via Fulcio keyless (task-107)"
                );
                record
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "jj squash: Fulcio keyless signing failed; falling back to local signing"
                );
                commit_signatures::sign_commit_local(
                    &repo_id,
                    &commit_sha,
                    &attribution,
                    &state.agent_signing_key,
                    &state.base_url,
                )
            }
        }
    } else {
        commit_signatures::sign_commit_local(
            &repo_id,
            &commit_sha,
            &attribution,
            &state.agent_signing_key,
            &state.base_url,
        )
    };

    state
        .commit_signatures
        .save(&record)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(record))
}

/// GET /api/v1/repos/:id/commits/:sha/signature
///
/// Return the stored commit signature for `(repo_id, sha)`. Lookup is scoped
/// by repo: a record created for repo A is not retrievable through repo B's
/// path.
pub async fn get_commit_signature(
    State(state): State<Arc<AppState>>,
    Path((repo_id, sha)): Path<(String, String)>,
) -> Result<Json<CommitSignature>, ApiError> {
    // Verify the repo exists.
    let _ = repo_path(&state, &repo_id).await?;

    state
        .commit_signatures
        .find(&repo_id, &sha)
        .await
        .map_err(ApiError::Internal)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no signature found for commit {sha}")))
}

/// GET /api/v1/repos/:id/commits/:sha/signature/verification
///
/// Verify the stored commit signature for `(repo_id, sha)` against the
/// server's configured trust anchors (task-107 plan item 3):
/// - fulcio records: (a) ECDSA signature over the commit SHA against the
///   leaf certificate, (b) certificate chain rooted in the CONFIGURED
///   Fulcio trust bundle within its validity window (F3/F5/F11), (c) leaf
///   SAN/CN matches the recorded OIDC subject, (d) a Rekor entry whose
///   hashedrekord body carries this commit's digest, signature, and
///   certificate (F4).
/// - local records: real Ed25519 verification of the signature over the
///   commit SHA against the forge signing key.
///
/// ABAC: resource_type = repo, action = read (registered in
/// abac_middleware.rs — F8).
pub async fn get_commit_signature_verification(
    State(state): State<Arc<AppState>>,
    Path((repo_id, sha)): Path<(String, String)>,
) -> Result<Json<crate::sigstore::SignatureVerificationResult>, ApiError> {
    // Verify the repo exists.
    let _ = repo_path(&state, &repo_id).await?;

    match crate::sigstore::verify_state_commit_signature(&state, &repo_id, &sha).await {
        Ok(Some(result)) => Ok(Json(result)),
        Ok(None) => Err(ApiError::NotFound(format!(
            "no signature found for commit {sha}"
        ))),
        Err(e) => Err(ApiError::Internal(e)),
    }
}

/// POST /api/v1/repos/:id/jj/undo
pub async fn jj_undo(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;
    state
        .jj_ops
        .jj_undo(&path)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/repos/:id/jj/bookmark
pub async fn jj_bookmark(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<BookmarkRequest>,
) -> Result<StatusCode, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    crate::abac::check_repo_abac(&state, &repo_id, &auth)
        .await
        .map_err(ApiError::Forbidden)?;
    state
        .jj_ops
        .jj_bookmark_create(&path, &req.name, &req.change_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::build_router(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_project_and_repo(app: Router) -> (Router, String) {
        let repo = serde_json::json!({ "name": "test-repo", "workspace_id": "test-ws" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&repo).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let repo_json = body_json(resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        (app, repo_id)
    }

    /// jj log returns empty list (NoopJjOps returns []).
    #[tokio::test]
    async fn jj_log_returns_empty_for_noop() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/jj/log"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    /// jj init returns 204 (NoopJjOps succeeds).
    #[tokio::test]
    async fn jj_init_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/init"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// jj new returns a change_id.
    #[tokio::test]
    async fn jj_new_returns_change_id() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let body = serde_json::json!({ "description": "test change" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/new"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["change_id"].as_str().is_some());
    }

    /// jj squash returns 200 with a commit signature (M13.8 Sigstore).
    #[tokio::test]
    async fn jj_squash_returns_commit_signature() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/squash"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["commit_sha"].as_str().is_some(), "missing commit_sha");
        assert!(json["signature"].as_str().is_some(), "missing signature");
        assert!(
            json["signing_key_id"].as_str().is_some(),
            "missing signing_key_id"
        );
        assert_eq!(json["algorithm"].as_str().unwrap(), "EdDSA");
        assert_eq!(json["sigstore_mode"].as_str().unwrap(), "local");
    }

    /// jj squash signature can be retrieved via GET /commits/:sha/signature (M13.8).
    #[tokio::test]
    async fn commit_signature_retrievable_after_squash() {
        use tower::ServiceExt as _;
        let state = crate::mem::test_state();
        let app = crate::build_router(state);
        let (app, repo_id) = create_project_and_repo(app).await;

        // First squash to produce a signature.
        let squash_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/squash"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(squash_resp.status(), StatusCode::OK);
        let squash_json = body_json(squash_resp).await;
        let sha = squash_json["commit_sha"].as_str().unwrap().to_string();

        // Then retrieve the signature by SHA.
        let sig_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/commits/{sha}/signature"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sig_resp.status(), StatusCode::OK);
        let sig_json = body_json(sig_resp).await;
        assert_eq!(sig_json["commit_sha"].as_str().unwrap(), sha);
        assert!(sig_json["signature"].as_str().is_some());
    }

    /// GET /commits/:sha/signature returns 404 for an unknown SHA (M13.8).
    #[tokio::test]
    async fn commit_signature_unknown_sha_returns_404() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/commits/deadbeef/signature"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// jj undo returns 204.
    #[tokio::test]
    async fn jj_undo_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/undo"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// jj bookmark returns 204.
    #[tokio::test]
    async fn jj_bookmark_returns_no_content() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let body = serde_json::json!({ "name": "my-feature", "change_id": "abc123" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/bookmark"))
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// Agent with ABAC policy blocking repo access gets 403 on jj_new (G6-A).
    #[tokio::test(flavor = "multi_thread")]
    async fn jj_new_abac_blocked_returns_403() {
        use crate::abac::AbacPolicy;
        use gyre_domain::Repository;
        use std::collections::HashMap;

        let state = crate::mem::test_state();

        // Create a repo with a known ID.
        let repo = Repository::new(
            gyre_common::Id::new("repo-jj-abac"),
            gyre_common::Id::new("proj-1"),
            "jj-abac-test",
            "/tmp/jj-abac-test",
            0,
        );
        state.repos.create(&repo).await.unwrap();

        // Set ABAC policy requiring scope=repo:special.
        // Agent JWTs have scope=agent — they won't satisfy this policy.
        {
            let mut required = HashMap::new();
            required.insert("scope".to_string(), "repo:special".to_string());
            let policies = vec![AbacPolicy {
                resource_type: "repo".to_string(),
                resource_id: None,
                required_claims: required,
            }];
            let json = serde_json::to_string(&policies).unwrap();
            state
                .kv_store
                .kv_set("abac_policies", "repo-jj-abac", json)
                .await
                .unwrap();
        }

        // Mint an agent JWT (scope=agent ≠ repo:special → ABAC will deny).
        let jwt = state
            .agent_signing_key
            .mint("agent-blocked", "task-1", "system", &state.base_url, 3600)
            .expect("mint JWT");

        // Register the JWT so the auth middleware accepts it.
        state
            .kv_store
            .kv_set("agent_tokens", "agent-blocked", jwt.clone())
            .await
            .unwrap();

        let app = crate::build_router(state);

        let body = serde_json::json!({ "description": "should be denied" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-jj-abac/jj/new")
                    .header("Authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// jj init on unknown repo returns 404.
    #[tokio::test]
    async fn jj_init_unknown_repo_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/nonexistent/jj/init")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// jj log with limit query parameter.
    #[tokio::test]
    async fn jj_log_with_limit() {
        let app = app();
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repos/{repo_id}/jj/log?limit=5"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
