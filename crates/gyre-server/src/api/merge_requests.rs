use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{MergeRequest, MrStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

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

#[derive(Serialize)]
pub struct MrResponse {
    pub id: String,
    pub repository_id: String,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: String,
    pub author_agent_id: Option<String>,
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
            created_at: mr.created_at,
            updated_at: mr.updated_at,
        }
    }
}

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

pub async fn create_mr(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateMrRequest>,
) -> Result<(StatusCode, Json<MrResponse>), ApiError> {
    let now = now_secs();
    let mut mr = MergeRequest::new(
        new_id(),
        Id::new(req.repository_id),
        req.title,
        req.source_branch,
        req.target_branch,
        now,
    );
    mr.author_agent_id = req.author_agent_id.map(Id::new);
    state.merge_requests.create(&mr).await?;
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
    mr.transition_status(new_status)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    mr.updated_at = now_secs();
    state.merge_requests.update(&mr).await?;
    Ok(Json(MrResponse::from(mr)))
}

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

        // Open -> Merged is invalid (must go through Approved)
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
}
