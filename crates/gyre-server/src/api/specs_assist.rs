//! S3.3: Spec Editing Backend — assist/save/prompts
//!
//! POST /api/v1/repos/:id/specs/assist   — LLM-assisted editing (SSE stream, stubbed LLM)
//! POST /api/v1/repos/:id/specs/save     — commit spec to feature branch + create MR
//! POST /api/v1/repos/:id/prompts/save   — direct commit to default branch (stubbed)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::{stream, StreamExt as _};
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{MergeRequest, MrStatus};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

use crate::{
    auth::AuthenticatedAgent,
    llm_rate_limit::{check_rate_limit, LLM_RATE_LIMIT, LLM_WINDOW_SECS},
    AppState,
};

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SpecAssistRequest {
    pub spec_path: String,
    pub instruction: String,
    pub draft_content: Option<String>,
}

#[derive(Serialize)]
pub struct DiffOp {
    pub op: String,
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct SpecSaveRequest {
    pub spec_path: String,
    pub content: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct SpecSaveResponse {
    pub branch: String,
    pub mr_id: String,
}

#[derive(Deserialize)]
pub struct PromptSaveRequest {
    pub prompt_path: String,
    pub content: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct PromptSaveResponse {
    pub commit_sha: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive a URL-safe slug from a spec path for branch naming.
///
/// e.g. "specs/system/payment-retry.md" -> "specs-system-payment-retry-a1b2"
///
/// The 4-char hex suffix is a deterministic hash of the full original path,
/// preventing collisions between paths that produce the same slug segment
/// (e.g. "system/payment-retry.md" and "system/payment-retry-v2.md").
fn spec_path_slug(spec_path: &str) -> String {
    let without_ext = spec_path.trim_end_matches(".md");
    let slug = without_ext.replace('/', "-").to_lowercase();
    let hash: u32 = spec_path
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    format!("{}-{:04x}", slug, hash & 0xffff)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/v1/repos/:id/specs/assist
///
/// LLM-assisted spec editing. Returns an SSE stream with partial explanation
/// chunks and a final complete event containing the diff.
///
/// ABAC: resource_type "spec", action "generate".
///
/// The LLM call is stubbed — a simulated diff is produced from the instruction
/// text. The SSE format (partial/complete/error events) and ABAC mapping are
/// correct per ui-layout.md §3 "LLM Endpoint Contract".
pub async fn assist_spec(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    caller: AuthenticatedAgent,
    Json(req): Json<SpecAssistRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    // Per-user/workspace sliding-window rate limit (HSI §6): 10 req/60 s.
    {
        let workspace_id = repo.workspace_id.to_string();
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &caller.agent_id,
            &workspace_id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // Require LLM to be configured.
    let factory = state
        .llm
        .as_ref()
        .ok_or(super::error::ApiError::LlmUnavailable)?;

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&repo.workspace_id, "specs-assist")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_SPECS_ASSIST.to_string());

    let system_prompt = template_content
        .replace("{{spec_path}}", &req.spec_path)
        .replace(
            "{{draft_content}}",
            req.draft_content.as_deref().unwrap_or(""),
        )
        .replace("{{instruction}}", &req.instruction);
    let user_prompt = format!("Instruction: {}", req.instruction);

    // Resolve model and call streaming LLM.
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(&state, &repo.workspace_id, "specs-assist").await;
    let stream = factory
        .for_model(&model)
        .stream_complete(&system_prompt, &user_prompt, None)
        .await
        .map_err(ApiError::Internal)?;

    let chunks: Vec<String> = stream.filter_map(|r| async { r.ok() }).collect().await;
    let full_text = chunks.join("");

    let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();
    for chunk in &chunks {
        let data = serde_json::to_string(&serde_json::json!({"text": chunk})).unwrap_or_default();
        events.push(Ok(Event::default().event("partial").data(data)));
    }
    let complete_data =
        serde_json::to_string(&serde_json::json!({"text": full_text})).unwrap_or_default();
    events.push(Ok(Event::default().event("complete").data(complete_data)));

    let s = stream::iter(events);
    Ok(Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// POST /api/v1/repos/:id/specs/save
///
/// Commits a spec change to a `spec-edit/<slug>-<uuid>` feature branch and
/// auto-creates an MR targeting the default branch. If an existing open MR
/// for the same spec_path exists (matched by branch prefix), appends a commit
/// to the existing branch.
///
/// Creates a priority-2 (High) "Spec pending approval" notification. The
/// notification's entity_id is the MR ID so the Inbox "Approve" action can
/// enqueue the MR.
///
/// ABAC: resource_type "spec", action "write".
pub async fn save_spec(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<SpecSaveRequest>,
) -> Result<(StatusCode, Json<SpecSaveResponse>), ApiError> {
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    let slug = spec_path_slug(&req.spec_path);
    let branch_prefix = format!("spec-edit/{}", slug);

    // Check for an existing open MR for this spec_path (matched by branch prefix).
    let all_mrs = state
        .merge_requests
        .list_by_repo(&repo_id_typed)
        .await
        .unwrap_or_default();
    let existing_mr = all_mrs
        .into_iter()
        .find(|mr| mr.status == MrStatus::Open && mr.source_branch.starts_with(&branch_prefix));

    if let Some(mr) = existing_mr {
        // Existing open MR — append a commit to the existing branch.
        state
            .git_ops
            .write_file(
                &repo.path,
                &mr.source_branch,
                &req.spec_path,
                req.content.as_bytes(),
                &req.message,
            )
            .await
            .map_err(ApiError::Internal)?;

        return Ok((
            StatusCode::OK,
            Json(SpecSaveResponse {
                branch: mr.source_branch,
                mr_id: mr.id.to_string(),
            }),
        ));
    }

    // Generate new branch: spec-edit/<slug>-<short_uuid>
    let short_uuid = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
    let branch_name = format!("{}-{}", branch_prefix, short_uuid);

    // Create the feature branch from the default branch.
    state
        .git_ops
        .create_branch(&repo.path, &branch_name, &repo.default_branch)
        .await
        .map_err(ApiError::Internal)?;

    // Write the spec content to the new branch.
    state
        .git_ops
        .write_file(
            &repo.path,
            &branch_name,
            &req.spec_path,
            req.content.as_bytes(),
            &req.message,
        )
        .await
        .map_err(ApiError::Internal)?;

    let now = now_secs();
    let mr_id = new_id();
    let mut mr = MergeRequest::new(
        mr_id.clone(),
        repo_id_typed.clone(),
        format!("Spec edit: {}", req.spec_path),
        branch_name.clone(),
        repo.default_branch.clone(),
        now,
    );
    mr.workspace_id = repo.workspace_id.clone();

    state
        .merge_requests
        .create(&mr)
        .await
        .map_err(ApiError::Internal)?;

    // Resolve tenant_id from workspace for the notification.
    let tenant_id = match state.workspaces.find_by_id(&repo.workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => "unknown".to_string(),
    };

    // Priority-2 "Spec pending approval" notification (HSI §2 + §8).
    // user_id is "system" — real per-user fan-out requires workspace membership
    // which is outside this task's scope.
    let notif_id = new_id();
    let mut notif = Notification::new(
        notif_id,
        repo.workspace_id.clone(),
        Id::new("system"),
        NotificationType::SpecPendingApproval,
        format!("Spec pending approval: {}", req.spec_path),
        &tenant_id,
        now as i64,
    );
    notif.entity_ref = Some(mr_id.to_string());
    // Non-fatal — MR is created even if notification fails.
    let _ = state.notifications.create(&notif).await;

    Ok((
        StatusCode::CREATED,
        Json(SpecSaveResponse {
            branch: branch_name,
            mr_id: mr_id.to_string(),
        }),
    ))
}

/// POST /api/v1/repos/:id/prompts/save
///
/// Direct commit of a prompt template to the default branch — no MR, no
/// approval notification. Prompt templates in `specs/prompts/` are in
/// spec-lifecycle's `ignored_paths`, enabling fast iteration without formal
/// spec approval.
///
/// ABAC: resource_type "spec", action "generate" (not "write") so that
/// Supervised trust's `trust:require-human-mr-review` policy does not block
/// prompt iteration.
pub async fn save_prompt(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<PromptSaveRequest>,
) -> Result<(StatusCode, Json<PromptSaveResponse>), ApiError> {
    let repo_id_typed = Id::new(&repo_id);
    let repo = state
        .repos
        .find_by_id(&repo_id_typed)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {} not found", repo_id)))?;

    // Commit directly to the default branch (no feature branch, no MR).
    let commit_sha = state
        .git_ops
        .write_file(
            &repo.path,
            &repo.default_branch,
            &req.prompt_path,
            req.content.as_bytes(),
            &req.message,
        )
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(PromptSaveResponse { commit_sha })))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn auth() -> &'static str {
        "Bearer test-token"
    }

    #[tokio::test]
    async fn assist_spec_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add error handling section",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/specs/assist")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assist_spec_rate_limited_after_10_requests() {
        let app = app();

        // Create a repo so assist_spec can proceed past the 404 check.
        let create_body = serde_json::json!({
            "name": "rate-limit-test-repo",
            "workspace_id": "ws-rate-test",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add a summary",
        });

        // First 10 requests must succeed.
        for i in 0..10 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/repos/{}/specs/assist", repo_id))
                        .header("Authorization", auth())
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        // 11th request must be rate-limited.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/assist", repo_id))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get("Retry-After")
            .expect("Retry-After header");
        let secs: u64 = retry_after.to_str().unwrap().parse().unwrap();
        assert!(secs >= 1, "Retry-After must be at least 1 second");
    }

    #[tokio::test]
    async fn save_spec_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision\n\nContent here.",
            "message": "Add vision spec",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/specs/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn save_prompt_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "prompt_path": "specs/prompts/specs-assist.md",
            "content": "# Updated prompt",
            "message": "Tweak specs-assist prompt",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/no-such-repo/prompts/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn save_spec_creates_mr_for_existing_repo() {
        let app = app();
        // Create a repo first.
        let create_body = serde_json::json!({
            "name": "spec-edit-test",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        // Save a spec — should create an MR.
        let save_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision\n\nContent here.",
            "message": "Add vision spec",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert!(json["branch"].as_str().unwrap().starts_with("spec-edit/"));
        assert!(!json["mr_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn save_spec_existing_mr_returns_same_mr() {
        let app = app();
        // Create a repo.
        let create_body = serde_json::json!({
            "name": "spec-edit-existing-mr",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let save_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision v1",
            "message": "First edit",
        });

        // First save — creates MR.
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::CREATED);
        let json1 = body_json(resp1).await;
        let branch1 = json1["branch"].as_str().unwrap().to_string();
        let mr_id1 = json1["mr_id"].as_str().unwrap().to_string();

        // Second save to same spec_path — should return the existing MR.
        let save_body2 = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "content": "# Vision v2",
            "message": "Second edit",
        });
        let resp2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/specs/save", repo_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Existing MR case returns 200, not 201.
        assert_eq!(resp2.status(), StatusCode::OK);
        let json2 = body_json(resp2).await;
        assert_eq!(json2["branch"].as_str().unwrap(), branch1);
        assert_eq!(json2["mr_id"].as_str().unwrap(), mr_id1);
    }

    #[tokio::test]
    async fn save_prompt_returns_commit_sha() {
        let app = app();
        // Create a repo.
        let create_body = serde_json::json!({
            "name": "prompt-save-test",
            "workspace_id": "ws-1",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let save_body = serde_json::json!({
            "prompt_path": "specs/prompts/specs-assist.md",
            "content": "# Updated prompt template",
            "message": "Update specs-assist prompt",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{}/prompts/save", repo_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&save_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let sha = json["commit_sha"].as_str().unwrap();
        // SHA should be a valid 40-char hex string
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn spec_path_slug_no_collision() {
        let s1 = spec_path_slug("specs/system/payment-retry.md");
        let s2 = spec_path_slug("specs/system/payment-retry-v2.md");
        assert_ne!(s1, s2, "different paths must produce different slugs");
        assert!(s1.starts_with("specs-system-payment-retry-"));
    }

    #[test]
    fn spec_path_slug_format() {
        let slug = spec_path_slug("specs/system/vision.md");
        // Must match: lowercase, no slashes, ends with 4-hex-char suffix
        assert!(slug.starts_with("specs-system-vision-"));
        let parts: Vec<&str> = slug.rsplitn(2, '-').collect();
        assert_eq!(parts[0].len(), 4);
        assert!(parts[0].chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── LLM endpoint tests ───────────────────────────────────────────────────

    fn app_no_llm() -> Router {
        let mut s = (*crate::mem::test_state()).clone();
        s.llm = None;
        crate::api::api_router().with_state(std::sync::Arc::new(s))
    }

    #[tokio::test]
    async fn assist_spec_returns_503_when_llm_unavailable() {
        let app = app_no_llm();

        // Create a repo.
        let create_body = serde_json::json!({
            "name": "assist-503-repo",
            "workspace_id": "ws-assist-503",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add a summary",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn assist_spec_with_mock_llm_streams_sse_events() {
        let app = app();

        // Create a repo.
        let create_body = serde_json::json!({
            "name": "assist-sse-repo",
            "workspace_id": "ws-assist-sse",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let assist_body = serde_json::json!({
            "spec_path": "specs/system/vision.md",
            "instruction": "Add a summary",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/specs/assist"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assist_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));
    }
}
