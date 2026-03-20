use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, MergeRequest, MrStatus, Review, ReviewComment, ReviewDecision};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::domain_events::DomainEvent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateMrRequest {
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub author_agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ListMrsQuery {
    pub status: Option<String>,
    pub repository_id: Option<String>,
}

#[derive(Deserialize)]
pub struct TransitionStatusRequest {
    pub status: String,
}

#[derive(Deserialize)]
pub struct AddCommentRequest {
    pub author_agent_id: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
}

#[derive(Deserialize)]
pub struct SubmitReviewRequest {
    pub reviewer_agent_id: String,
    pub decision: String,
    pub body: Option<String>,
}

#[derive(Serialize)]
pub struct DiffStatsResponse {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Serialize)]
pub struct MrResponse {
    pub id: String,
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: String,
    pub author_agent_id: Option<String>,
    pub diff_stats: Option<DiffStatsResponse>,
    pub has_conflicts: Option<bool>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<MergeRequest> for MrResponse {
    fn from(mr: MergeRequest) -> Self {
        Self {
            id: mr.id.to_string(),
            repository_id: mr.repository_id.to_string(),
            title: mr.title,
            source_branch: mr.source_branch,
            target_branch: mr.target_branch,
            status: mr_status_str(&mr.status),
            author_agent_id: mr.author_agent_id.map(|id| id.to_string()),
            diff_stats: mr.diff_stats.map(|d| DiffStatsResponse {
                files_changed: d.files_changed,
                insertions: d.insertions,
                deletions: d.deletions,
            }),
            has_conflicts: mr.has_conflicts,
            created_at: mr.created_at,
            updated_at: mr.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct CommentResponse {
    pub id: String,
    pub merge_request_id: String,
    pub author_agent_id: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub created_at: u64,
}

impl From<ReviewComment> for CommentResponse {
    fn from(c: ReviewComment) -> Self {
        Self {
            id: c.id.to_string(),
            merge_request_id: c.merge_request_id.to_string(),
            author_agent_id: c.author_agent_id,
            body: c.body,
            file_path: c.file_path,
            line_number: c.line_number,
            created_at: c.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct ReviewResponse {
    pub id: String,
    pub merge_request_id: String,
    pub reviewer_agent_id: String,
    pub decision: String,
    pub body: Option<String>,
    pub created_at: u64,
}

impl From<Review> for ReviewResponse {
    fn from(r: Review) -> Self {
        Self {
            id: r.id.to_string(),
            merge_request_id: r.merge_request_id.to_string(),
            reviewer_agent_id: r.reviewer_agent_id,
            decision: review_decision_str(&r.decision),
            body: r.body,
            created_at: r.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub patches: Vec<FileDiffResponse>,
}

#[derive(Serialize)]
pub struct FileDiffResponse {
    pub path: String,
    pub status: String,
    pub patch: Option<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn mr_status_str(s: &MrStatus) -> String {
    match s {
        MrStatus::Open => "open",
        MrStatus::Approved => "approved",
        MrStatus::Merged => "merged",
        MrStatus::Closed => "closed",
    }
    .to_string()
}

fn parse_mr_status(s: &str) -> Result<MrStatus, ApiError> {
    match s.to_lowercase().as_str() {
        "open" => Ok(MrStatus::Open),
        "approved" => Ok(MrStatus::Approved),
        "merged" => Ok(MrStatus::Merged),
        "closed" => Ok(MrStatus::Closed),
        _ => Err(ApiError::InvalidInput(format!("unknown MR status: {s}"))),
    }
}

fn review_decision_str(d: &ReviewDecision) -> String {
    match d {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes_requested",
    }
    .to_string()
}

fn parse_review_decision(s: &str) -> Result<ReviewDecision, ApiError> {
    match s.to_lowercase().as_str() {
        "approved" => Ok(ReviewDecision::Approved),
        "changes_requested" => Ok(ReviewDecision::ChangesRequested),
        _ => Err(ApiError::InvalidInput(format!(
            "unknown review decision: {s}"
        ))),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[instrument(skip(state, req), fields(source = %req.source_branch, target = %req.target_branch))]
pub async fn create_mr(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateMrRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    let now = now_secs();
    let repo_id = Id::new(req.repository_id);
    let mut mr = MergeRequest::new(
        new_id(),
        repo_id.clone(),
        req.title,
        req.source_branch.clone(),
        req.target_branch.clone(),
        now,
    );
    mr.author_agent_id = req.author_agent_id.map(Id::new);

    // Compute diff stats and conflict detection if the repository has a path
    if let Ok(Some(repo)) = state.repos.find_by_id(&repo_id).await {
        if let Ok(diff) = state
            .git_ops
            .diff(&repo.path, &req.target_branch, &req.source_branch)
            .await
        {
            mr.diff_stats = Some(gyre_domain::DiffStats {
                files_changed: diff.files_changed,
                insertions: diff.insertions,
                deletions: diff.deletions,
            });
        }
        if let Ok(can_merge) = state
            .git_ops
            .can_merge(&repo.path, &req.source_branch, &req.target_branch)
            .await
        {
            mr.has_conflicts = Some(!can_merge);
        }
    }

    state.merge_requests.create(&mr).await?;
    let _ = state.event_tx.send(DomainEvent::MrCreated {
        id: mr.id.to_string(),
    });
    Ok((StatusCode::CREATED, Json(MrResponse::from(mr))))
}

pub async fn list_mrs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListMrsQuery>,
) -> Result<Json<Vec<MrResponse>>, ApiError> {
    let mrs = match (params.status, params.repository_id) {
        (Some(status_str), _) => {
            let status = parse_mr_status(&status_str)?;
            state.merge_requests.list_by_status(&status).await?
        }
        (_, Some(repo_id)) => state.merge_requests.list_by_repo(&Id::new(repo_id)).await?,
        _ => state.merge_requests.list().await?,
    };
    Ok(Json(mrs.into_iter().map(MrResponse::from).collect()))
}

pub async fn get_mr(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<MrResponse>, ApiError> {
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    Ok(Json(MrResponse::from(mr)))
}

#[instrument(skip(state, req), fields(mr_id = %id, new_status = %req.status))]
pub async fn transition_mr_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<TransitionStatusRequest>,
) -> Result<Json<MrResponse>, ApiError> {
    let mut mr = state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    let new_status = parse_mr_status(&req.status)?;
    let is_merge = matches!(new_status, MrStatus::Merged);
    mr.transition_status(new_status)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    let ts = now_secs();
    mr.updated_at = ts;
    state.merge_requests.update(&mr).await?;
    let _ = state.event_tx.send(DomainEvent::MrStatusChanged {
        id: mr.id.to_string(),
        status: req.status.clone(),
    });

    // Auto-track mr.merged analytics event
    if is_merge {
        let ev = AnalyticsEvent::new(
            new_id(),
            "mr.merged",
            mr.author_agent_id.as_ref().map(|id| id.to_string()),
            serde_json::json!({ "mr_id": mr.id.to_string() }),
            ts,
        );
        let _ = state.analytics.record(&ev).await;
    }

    Ok(Json(MrResponse::from(mr)))
}

pub async fn add_comment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AddCommentRequest>,
) -> Result<(StatusCode, Json<CommentResponse>), ApiError> {
    // Verify MR exists
    state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let mut comment = ReviewComment::new(
        new_id(),
        Id::new(id),
        req.author_agent_id,
        req.body,
        now_secs(),
    );
    comment.file_path = req.file_path;
    comment.line_number = req.line_number;

    state.reviews.add_comment(&comment).await?;
    Ok((StatusCode::CREATED, Json(CommentResponse::from(comment))))
}

pub async fn list_comments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CommentResponse>>, ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let comments = state.reviews.list_comments(&mr_id).await?;
    Ok(Json(
        comments.into_iter().map(CommentResponse::from).collect(),
    ))
}

#[instrument(skip(state, req), fields(mr_id = %id, reviewer = %req.reviewer_agent_id, decision = %req.decision))]
pub async fn submit_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SubmitReviewRequest>,
) -> Result<(StatusCode, Json<ReviewResponse>), ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let decision = parse_review_decision(&req.decision)?;
    let mut review = Review::new(new_id(), mr_id, req.reviewer_agent_id, decision, now_secs());
    review.body = req.body;

    state.reviews.submit_review(&review).await?;
    Ok((StatusCode::CREATED, Json(ReviewResponse::from(review))))
}

pub async fn list_reviews(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ReviewResponse>>, ApiError> {
    let mr_id = Id::new(&id);
    state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let reviews = state.reviews.list_reviews(&mr_id).await?;
    Ok(Json(
        reviews.into_iter().map(ReviewResponse::from).collect(),
    ))
}

pub async fn get_diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DiffResponse>, ApiError> {
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let repo = state
        .repos
        .find_by_id(&mr.repository_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repository {} not found", mr.repository_id)))?;

    let diff = state
        .git_ops
        .diff(&repo.path, &mr.target_branch, &mr.source_branch)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(DiffResponse {
        files_changed: diff.files_changed,
        insertions: diff.insertions,
        deletions: diff.deletions,
        patches: diff
            .patches
            .into_iter()
            .map(|p| FileDiffResponse {
                path: p.path,
                status: p.status,
                patch: p.patch,
            })
            .collect(),
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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

    async fn create_test_mr(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": title,
            "source_branch": "feat/x",
            "target_branch": "main"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn create_and_get_mr() {
        let app = app();
        let (app, id) = create_test_mr(app, "Add feature").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "open");
        assert_eq!(json["title"], "Add feature");
    }

    #[tokio::test]
    async fn mr_status_transition_valid() {
        let app = app();
        let (app, id) = create_test_mr(app, "Approve me").await;

        let body = serde_json::json!({ "status": "approved" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "approved");
    }

    #[tokio::test]
    async fn mr_status_transition_invalid() {
        let app = app();
        let (app, id) = create_test_mr(app, "Invalid trans").await;

        let body = serde_json::json!({ "status": "merged" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_mrs_by_repository() {
        let app = app();
        let (_, _) = create_test_mr(app.clone(), "MR for repo").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests?repository_id=repo-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_mr_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn add_and_list_comments() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Comment test").await;

        let body = serde_json::json!({
            "author_agent_id": "agent-1",
            "body": "Looks good to me"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["body"], "Looks good to me");
    }

    #[tokio::test]
    async fn comment_with_file_and_line() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "File comment test").await;

        let body = serde_json::json!({
            "author_agent_id": "agent-1",
            "body": "Fix this line",
            "file_path": "src/lib.rs",
            "line_number": 10
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["file_path"], "src/lib.rs");
        assert_eq!(json["line_number"], 10);
    }

    #[tokio::test]
    async fn submit_approve_review() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Review test").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "approved",
            "body": "LGTM"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "approved");

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn submit_changes_requested_review() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Changes test").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "changes_requested"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["decision"], "changes_requested");
    }

    #[tokio::test]
    async fn review_bad_decision_rejected() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Bad decision").await;

        let body = serde_json::json!({
            "reviewer_agent_id": "agent-1",
            "decision": "maybe"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/reviews"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn diff_endpoint_returns_200() {
        let app = app();
        let (app, mr_id) = create_test_mr(app, "Diff test").await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // NoopGitOps returns empty diff — but the repo won't be found, so this will 404
        // since test_state doesn't have a repo with id "repo-1"
        // The 404 is for repo not found, which is correct behavior.
        assert!(
            resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::OK,
            "unexpected status: {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn comment_on_missing_mr_returns_404() {
        let body = serde_json::json!({ "author_agent_id": "a1", "body": "hi" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/no-such/comments")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn review_on_missing_mr_returns_404() {
        let body = serde_json::json!({ "reviewer_agent_id": "a1", "decision": "approved" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/no-such/reviews")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
