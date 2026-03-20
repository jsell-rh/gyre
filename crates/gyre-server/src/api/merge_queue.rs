use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct EnqueueRequest {
    pub merge_request_id: String,
    pub priority: Option<u32>,
}

#[derive(Serialize)]
pub struct QueueEntryResponse {
    pub id: String,
    pub merge_request_id: String,
    pub priority: u32,
    pub status: String,
    pub enqueued_at: u64,
    pub processed_at: Option<u64>,
    pub error_message: Option<String>,
}

impl From<MergeQueueEntry> for QueueEntryResponse {
    fn from(e: MergeQueueEntry) -> Self {
        Self {
            id: e.id.to_string(),
            merge_request_id: e.merge_request_id.to_string(),
            priority: e.priority,
            status: status_str(&e.status),
            enqueued_at: e.enqueued_at,
            processed_at: e.processed_at,
            error_message: e.error_message,
        }
    }
}

fn status_str(s: &MergeQueueEntryStatus) -> String {
    match s {
        MergeQueueEntryStatus::Queued => "queued",
        MergeQueueEntryStatus::Processing => "processing",
        MergeQueueEntryStatus::Merged => "merged",
        MergeQueueEntryStatus::Failed => "failed",
        MergeQueueEntryStatus::Cancelled => "cancelled",
    }
    .to_string()
}

#[instrument(skip(state, req), fields(mr_id = %req.merge_request_id))]
pub async fn enqueue(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnqueueRequest>,
) -> Result<(StatusCode, Json<QueueEntryResponse>), ApiError> {
    let priority = req.priority.unwrap_or(50);
    let mr_id = Id::new(req.merge_request_id);
    let entry = MergeQueueEntry::new(new_id(), mr_id.clone(), priority, now_secs());
    state.merge_queue.enqueue(&entry).await?;

    // Trigger quality gate execution if this MR has an associated repo with gates.
    if let Ok(Some(mr)) = state.merge_requests.find_by_id(&mr_id).await {
        crate::gate_executor::trigger_gates_for_mr(state.clone(), mr_id, mr.repository_id).await;
    }

    Ok((StatusCode::CREATED, Json(QueueEntryResponse::from(entry))))
}

pub async fn list_queue(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<QueueEntryResponse>>, ApiError> {
    let entries = state.merge_queue.list_queue().await?;
    Ok(Json(
        entries.into_iter().map(QueueEntryResponse::from).collect(),
    ))
}

pub async fn cancel_entry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let entry = state
        .merge_queue
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("queue entry {id} not found")))?;

    if entry.is_terminal() {
        return Err(ApiError::InvalidInput(format!(
            "cannot cancel entry in terminal state: {}",
            status_str(&entry.status)
        )));
    }

    state.merge_queue.cancel(&Id::new(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
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

    async fn enqueue_entry(app: Router, mr_id: &str, priority: Option<u32>) -> (Router, String) {
        let mut body = serde_json::json!({ "merge_request_id": mr_id });
        if let Some(p) = priority {
            body["priority"] = serde_json::json!(p);
        }
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn enqueue_returns_created() {
        let app = app();
        let (_, id) = enqueue_entry(app, "mr-1", None).await;
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn enqueue_with_priority() {
        let app = app();
        let body = serde_json::json!({ "merge_request_id": "mr-1", "priority": 100 });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["priority"], 100);
        assert_eq!(json["status"], "queued");
    }

    #[tokio::test]
    async fn list_queue_initially_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-queue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_queue_shows_enqueued() {
        let app = app();
        let (app, _) = enqueue_entry(app.clone(), "mr-1", Some(50)).await;
        let (app, _) = enqueue_entry(app.clone(), "mr-2", Some(75)).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-queue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cancel_queued_entry() {
        let app = app();
        let (app, id) = enqueue_entry(app, "mr-1", None).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/merge-queue/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn cancel_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/merge-queue/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
