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
///
/// Creates the new change and signs its commit (task-264,
/// identity-security.md §Layer 3): every commit-producing surface produces
/// a persisted `CommitSignature`. Signing follows `GYRE_SIGNING_MODE`
/// (keyless Fulcio by default, local forge key by explicit configuration,
/// `none` only as an explicit operator opt-in). In keyless mode a caller
/// without a presentable OIDC JWT is refused (403), and a Fulcio failure
/// fails the operation (502) — never a silent local-key fallback.
pub async fn jj_new(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<NewChangeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/jj/new",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }
    let new_change = state
        .jj_ops
        .jj_new(&path, &req.description)
        .await
        .map_err(ApiError::Internal)?;

    // task-264: sign the new change's commit — the same shared entry point
    // every commit surface uses. `none` mode skips signing (explicit
    // operator opt-in); keyless failures surface as 403/502 errors.
    let attribution = commit_signatures::SigningAttribution::from_auth(&auth);
    match commit_signatures::sign_commit(
        &state,
        &repo_id,
        &new_change.commit_id,
        &attribution,
        auth.bearer_token.as_deref(),
    )
    .await
    {
        Ok(Some(record)) => {
            state
                .commit_signatures
                .save(&record)
                .await
                .map_err(ApiError::Internal)?;
        }
        Ok(None) => {}
        Err(e) => return Err(e.into()),
    }

    Ok(Json(serde_json::json!({
        "change_id": new_change.change_id,
        "commit_id": new_change.commit_id,
    })))
}

/// POST /api/v1/repos/:id/jj/squash
///
/// Squashes the working copy into its parent change and signs the resulting
/// commit. Signing mode comes from `GYRE_SIGNING_MODE` (keyless Fulcio is
/// the default; `local` and `none` are explicit operator opt-ins):
/// - `fulcio`: keyless Sigstore signing — Fulcio certificate issued against
///   the caller's OIDC JWT, the canonical attribution-bound payload signed
///   with the ephemeral key, the signature recorded in Rekor (task-107,
///   task-264). A Fulcio/Rekor failure FAILS the squash with an explicit
///   error naming the signing mode — there is no silent fallback to the
///   local forge key (task-264). A caller with no presentable OIDC JWT is
///   refused (403) rather than presenting an empty token to Fulcio.
/// - `none`: no signature record.
/// - `local` (explicit configuration only): forge Ed25519 key (M13.8).
pub async fn jj_squash(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<CommitSignature>, ApiError> {
    let path = repo_path(&state, &repo_id).await?;
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/jj/squash",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }
    let commit_sha = state
        .jj_ops
        .jj_squash(&path)
        .await
        .map_err(ApiError::Internal)?;

    // Attribution comes from the caller's validated JWT claims — never
    // placeholder literals (task-107 F2). Non-JWT callers (dev token, API
    // key) fall back to the resolved agent id with empty task/user fields.
    let attribution = commit_signatures::SigningAttribution::from_auth(&auth);

    // task-264: the shared signing entry point — keyless by default, no
    // silent local-key fallback, refusal for callers with no presentable
    // OIDC identity.
    let record = match commit_signatures::sign_commit(
        &state,
        &repo_id,
        &commit_sha,
        &attribution,
        auth.bearer_token.as_deref(),
    )
    .await
    .map_err(ApiError::from)?
    {
        Some(record) => record,
        None => {
            // `none` mode: skip signing entirely — no record is produced.
            // The response reports the commit with an empty signature so
            // callers can distinguish "unsigned" from "signed" without a
            // second lookup.
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
    };

    if record.sigstore_mode == gyre_ports::SigstoreMode::Fulcio {
        tracing::info!(
            commit_sha = %commit_sha,
            rekor_entry_id = ?record.rekor_entry_id,
            "jj squash: commit signed via Fulcio keyless (task-107)"
        );
    }

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
    // Verify the repo exists (errors propagate).
    repo_path(&state, &repo_id).await?;

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
    // Verify the repo exists (errors propagate).
    repo_path(&state, &repo_id).await?;

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
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/jj/undo",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }
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
    if let Err(reason) = crate::abac::check_repo_abac(&state, &repo_id, &auth).await {
        // task-104: AbacDenied audit event — per-handler repo-level ABAC
        // denial outside the ABAC middleware.
        crate::abac_middleware::record_abac_denied(
            &state,
            &auth,
            "repo",
            "write",
            None,
            "POST /api/v1/repos/:id/jj/bookmark",
        )
        .await;
        return Err(ApiError::Forbidden(reason));
    }
    state
        .jj_ops
        .jj_bookmark_create(&path, &req.name, &req.change_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::commit_signatures;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use std::sync::Arc;
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

    /// task-264 (identity-security.md §Layer 3): jj new is a commit-producing
    /// surface — the new change's commit must carry a persisted
    /// CommitSignature record, retrievable through the signature endpoint
    /// and VALIDATED by the verification endpoint (real Ed25519 check
    /// against the forge key, local mode). Before task-264, jj new produced
    /// no signature at all.
    #[tokio::test(flavor = "multi_thread")]
    async fn jj_new_signs_commit_with_persisted_validated_signature() {
        let state = crate::mem::test_state();
        let app = crate::build_router(state.clone());
        let (app, repo_id) = create_project_and_repo(app).await;

        let body = serde_json::json!({ "description": "signed change" });
        let resp = app
            .clone()
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
        let commit_id = json["commit_id"]
            .as_str()
            .expect("jj new response carries the signed commit id")
            .to_string();

        // The signature record is persisted and retrievable repo-scoped.
        let sig_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/commits/{commit_id}/signature"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            sig_resp.status(),
            StatusCode::OK,
            "jj new must produce a persisted CommitSignature (task-264)"
        );
        let sig_json = body_json(sig_resp).await;
        assert_eq!(sig_json["commit_sha"].as_str().unwrap(), commit_id);
        assert_eq!(sig_json["sigstore_mode"].as_str().unwrap(), "local");

        // The existing verification endpoint validates it — the real
        // Ed25519 check, not a 200-only probe.
        let ver_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repos/{repo_id}/commits/{commit_id}/signature/verification"
                    ))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ver_resp.status(), StatusCode::OK);
        let ver_json = body_json(ver_resp).await;
        assert!(ver_json["valid"].as_bool().unwrap());
        assert!(ver_json["signature_valid"].as_bool().unwrap());
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

    // ── Signing mode handler coverage (task-107, task-264) ───────────────────
    //
    // These exercise the jj HANDLER wiring, not the sigstore module
    // (whose signing/verification phases have their own suite in
    // sigstore.rs over an upstream-contract mock stack):
    // - attribution (F2) is threaded from the caller's validated JWT claims
    //   into the stored record with DISTINCT values, so a regression to
    //   placeholder literals fails the assertion.
    // - task-264: fulcio mode with an unreachable Fulcio FAILS LOUDLY — an
    //   explicit 502 naming the signing mode, no local-key record — and a
    //   non-JWT caller is refused (403) instead of presenting an empty
    //   token to Fulcio and silently degrading to local signing.

    /// task-264 (the task's verification intent, verbatim): fulcio mode +
    /// unreachable Fulcio + a real presentable JWT → the squash must FAIL
    /// with an explicit error naming the signing mode, and NO record signed
    /// with the local forge key may be produced for the operation. The old
    /// behavior logged "Fulcio keyless signing failed; falling back to local
    /// signing" and returned a local-key record with 200.
    #[tokio::test(flavor = "multi_thread")]
    async fn jj_squash_fulcio_unreachable_fails_loudly_no_local_fallback() {
        let mut state = crate::mem::test_state();
        // Configure fulcio mode pointing at a host that can never be
        // connected to: `fulcio.invalid` is the RFC 6761 reserved TLD that
        // must never resolve (CI: DNS lookup fails immediately); in this
        // sandbox it resolves to a sinkhole IP that refuses connections
        // (verified: connect refused in single-digit ms). Either way the
        // production transport's tokio::time::timeout and error path are
        // exercised for real.
        //
        // Arc::get_mut: test_state() returns a fresh Arc with refcount 1
        // (its internal clones were dropped), so the state can be
        // reconfigured before the router takes a clone.
        let state_mut = Arc::get_mut(&mut state).expect("sole owner");
        state_mut.signing_config = crate::commit_signatures::SigningConfig {
            mode: commit_signatures::SigningMode::Fulcio,
            fulcio_url: "https://fulcio.invalid".to_string(),
            rekor_url: "https://rekor.invalid".to_string(),
        };
        let app = crate::build_router(state.clone());

        // Production seeds the built-in ABAC policies at startup; the agent
        // JWT below carries UserRole::Agent and is allowed by
        // `builtin-agent-scoped-access` (write on any resource type). The
        // bare test state has an empty policy store, which would 403 the
        // request before the signing path under test is ever reached.
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");

        let (app, repo_id) = create_project_and_repo(app).await;

        // Mint and register a real agent JWT (presentable to Fulcio) with
        // DISTINCT attribution values, so a silent local fallback would be
        // observable in the record's fields.
        let jwt = state
            .agent_signing_key
            .mint(
                "agent-fallback",
                "task-77",
                "user-fallback",
                &state.base_url,
                3600,
                "default",
                "test-ws",
                &repo_id,
                "test-repo",
            )
            .expect("mint JWT");
        state
            .kv_store
            .kv_set("agent_tokens", "agent-fallback", jwt.clone())
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/jj/squash"))
                    .header("Authorization", format!("Bearer {jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // FAILS LOUDLY: explicit error, never a 200-with-local-record.
        assert_eq!(
            resp.status(),
            StatusCode::BAD_GATEWAY,
            "unreachable Fulcio with keyless mode must fail the squash explicitly"
        );
        let err_json = body_json(resp).await;
        let error = err_json["error"].as_str().unwrap();
        assert!(
            error.contains("fulcio"),
            "the response must identify the actual signing mode used: {error}"
        );

        // No local-key record was produced for this operation: the squash
        // commit SHA (the NoopJjOps double's fixed squash SHA) has no
        // persisted signature at all.
        let squash_sha = "0000000000000000000000000000000000000000";
        let record = state
            .commit_signatures
            .find(&repo_id, squash_sha)
            .await
            .unwrap();
        assert!(
            record.is_none(),
            "no record signed with the local forge key may be produced for a \
             failed keyless operation"
        );
    }

    /// task-264: fulcio mode with a non-JWT bearer (dev token) — there is
    /// no OIDC identity to present to Fulcio, so keyless signing must be
    /// REFUSED with an explicit error (403). The old behavior presented an
    /// empty bearer token (guaranteeing Fulcio failure) and silently fell
    /// back to local signing.
    #[tokio::test(flavor = "multi_thread")]
    async fn jj_squash_fulcio_without_jwt_is_refused() {
        let mut state = crate::mem::test_state();
        let state_mut = Arc::get_mut(&mut state).expect("sole owner");
        state_mut.signing_config = crate::commit_signatures::SigningConfig {
            mode: commit_signatures::SigningMode::Fulcio,
            fulcio_url: "https://fulcio.invalid".to_string(),
            rekor_url: "https://rekor.invalid".to_string(),
        };
        let app = crate::build_router(state.clone());
        let (app, repo_id) = create_project_and_repo(app).await;

        // Global dev token: agent_id "system", no JWT claims, no bearer JWT
        // usable by Fulcio.
        let resp = app
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
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "keyless signing without a presentable JWT must be refused, not fallen back"
        );
        let err_json = body_json(resp).await;
        let error = err_json["error"].as_str().unwrap();
        assert!(
            error.contains("no presentable OIDC identity"),
            "the refusal must name the missing identity: {error}"
        );
        // No local-key record was produced for the refusal.
        let squash_sha = "0000000000000000000000000000000000000000";
        assert!(state
            .commit_signatures
            .find(&repo_id, squash_sha)
            .await
            .unwrap()
            .is_none());
    }

    /// `none` mode: squash succeeds with an empty unsigned record and no
    /// record is persisted (the verification endpoint 404s).
    #[tokio::test(flavor = "multi_thread")]
    async fn jj_squash_none_mode_skips_signing() {
        let mut state = crate::mem::test_state();
        let state_mut = Arc::get_mut(&mut state).expect("sole owner");
        state_mut.signing_config = crate::commit_signatures::SigningConfig {
            mode: commit_signatures::SigningMode::None,
            fulcio_url: "https://fulcio.invalid".to_string(),
            rekor_url: "https://rekor.invalid".to_string(),
        };
        let app = crate::build_router(state.clone());
        let (app, repo_id) = create_project_and_repo(app).await;

        let resp = app
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
        assert_eq!(resp.status(), StatusCode::OK);
        let sig_json = body_json(resp).await;
        assert_eq!(sig_json["signature"].as_str().unwrap(), "");
        assert_eq!(sig_json["sigstore_mode"].as_str().unwrap(), "local");
        let sha = sig_json["commit_sha"].as_str().unwrap().to_string();

        // No record persisted: signature lookup 404s.
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
        assert_eq!(sig_resp.status(), StatusCode::NOT_FOUND);
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

        // Mint an agent JWT (repo scope ≠ the policy's repo:special claim →
        // ABAC will deny).
        let jwt = state
            .agent_signing_key
            .mint(
                "agent-blocked",
                "task-1",
                "system",
                &state.base_url,
                3600,
                "t1",
                "ws-1",
                "repo-jj-abac",
                "jj-abac",
            )
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
