use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus, MrStatus};
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
    let (entry, created) = enqueue_core(&state, &mr_id, priority).await?;
    let code = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((code, Json(QueueEntryResponse::from(entry))))
}

/// Shared enqueue core (agent-runtime.md §1 Phase 6): put a mergeable MR on
/// the queue and trigger gate execution for it. Used by the REST handler
/// and by the Ralph loop's automatic post-completion enqueue so both
/// surfaces apply the same mergeable-status guard and gate trigger.
///
/// Idempotent per open MR: the Ralph loop auto-enqueues every
/// agent-authored completion MR (`ralph_loop::on_agent_mr_created`), so a
/// manual enqueue of an MR that already has a non-terminal (Queued or
/// Processing) queue entry returns that existing entry instead of
/// inserting a duplicate — a duplicate could never be merged a second
/// time ("cannot re-merge"). A terminal entry (Merged/Failed/Cancelled)
/// does not block a fresh enqueue of a still-open MR: that is the retry
/// path for a previously failed entry. Returns `(entry, created)` so the
/// REST surface can answer 201 Created vs 200 OK.
pub(crate) async fn enqueue_core(
    state: &Arc<AppState>,
    mr_id: &Id,
    priority: u32,
) -> Result<(MergeQueueEntry, bool), ApiError> {
    // Verify the MR exists before enqueueing — prevents a FK constraint violation (500)
    // when the inbox retry button sends a fake or stale MR ID.
    let mr = state
        .merge_requests
        .find_by_id(mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {} not found", mr_id)))?;
    // Only a mergeable MR may enter the queue (task-095 R3-F2,
    // platform-model.md §6): the recovery protocol marks an MR `Reverted`,
    // and §6's "removed from the merge queue permanently" is silently
    // void if a Reverted / Merged / Closed MR can be re-enqueued and
    // re-merged. Resubmission goes through a fresh Open MR instead.
    if !matches!(mr.status, MrStatus::Open | MrStatus::Approved) {
        return Err(ApiError::Conflict(format!(
            "merge request {} cannot be enqueued (status: {:?}); only Open or Approved MRs may enter the merge queue",
            mr_id, mr.status
        )));
    }

    // Already-queued guard (idempotency, see the doc comment above):
    // list_queue() returns exactly the non-terminal entries, so finding
    // this MR there means it is already Queued or Processing. Gates are
    // NOT re-triggered in that case — the enqueue that created the entry
    // already triggered them, and trigger_gates_for_mr would insert a
    // second set of Pending GateResult rows for the same MR.
    if let Some(existing) = state
        .merge_queue
        .list_queue()
        .await?
        .into_iter()
        .find(|e| e.merge_request_id == *mr_id)
    {
        return Ok((existing, false));
    }

    let entry = MergeQueueEntry::new(new_id(), mr_id.clone(), priority, now_secs());
    state.merge_queue.enqueue(&entry).await?;

    // Trigger quality gate execution if this MR has an associated repo with gates.
    crate::gate_executor::trigger_gates_for_mr(state.clone(), mr_id.clone(), mr.repository_id)
        .await;

    Ok((entry, true))
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
    use gyre_common::Id;
    use gyre_domain::MergeRequest;
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::AppState;

    async fn make_app() -> (Router, Arc<AppState>) {
        let state = test_state();
        let router = crate::api::api_router().with_state(state.clone());
        (router, state)
    }

    /// Create a real MR in the in-memory repo so enqueue can verify it exists.
    async fn seed_mr(state: &Arc<AppState>, mr_id: &str) {
        let mr = MergeRequest::new(
            Id::new(mr_id),
            Id::new("repo-1"),
            Id::new("default"),
            "Test MR",
            "feat/test",
            "main",
            0,
        );
        state.merge_requests.create(&mr).await.unwrap();
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn enqueue_entry(
        app: Router,
        state: &Arc<AppState>,
        mr_id: &str,
        priority: Option<u32>,
    ) -> (Router, String) {
        seed_mr(state, mr_id).await;
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
        let (app, state) = make_app().await;
        let (_, id) = enqueue_entry(app, &state, "mr-1", None).await;
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn enqueue_with_priority() {
        let (app, state) = make_app().await;
        seed_mr(&state, "mr-1").await;
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
    async fn enqueue_nonexistent_mr_returns_404() {
        let (app, _state) = make_app().await;
        let body = serde_json::json!({ "merge_request_id": "does-not-exist" });
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
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// Idempotency (task-118 review): the Ralph loop auto-enqueues every
    /// agent completion MR (`ralph_loop::on_agent_mr_created`), so a manual
    /// enqueue of an already-queued MR must return the existing entry — not
    /// a duplicate the merge processor could never merge a second time
    /// ("cannot re-merge").
    #[tokio::test]
    async fn enqueue_is_idempotent_per_open_mr() {
        let (app, state) = make_app().await;
        seed_mr(&state, "mr-1").await;
        let body = serde_json::to_vec(&serde_json::json!({ "merge_request_id": "mr-1" })).unwrap();

        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);
        let first_json = body_json(first).await;

        let second = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        let second_json = body_json(second).await;

        assert_eq!(
            first_json["id"], second_json["id"],
            "re-enqueue returns the existing queue entry"
        );
        assert_eq!(second_json["status"], "queued");

        let entries = state.merge_queue.list_queue().await.unwrap();
        assert_eq!(entries.len(), 1, "no duplicate queue entry for one MR");
    }

    /// A terminal (Failed) entry must not block a fresh enqueue of a
    /// still-open MR — that is the retry path for a failed entry.
    #[tokio::test]
    async fn enqueue_after_terminal_entry_creates_fresh_entry() {
        let (app, state) = make_app().await;
        let (app, id) = enqueue_entry(app, &state, "mr-1", None).await;

        // The first attempt failed terminally.
        state
            .merge_queue
            .update_status(
                &Id::new(&id),
                gyre_domain::MergeQueueEntryStatus::Failed,
                Some("merge conflict".to_string()),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "merge_request_id": "mr-1" }))
                            .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_ne!(
            json["id"].as_str().unwrap(),
            id,
            "retry after a terminal entry creates a fresh queue entry"
        );

        // Only the fresh non-terminal entry is listed.
        let entries = state.merge_queue.list_queue().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id.as_str(), json["id"].as_str().unwrap());
    }

    #[tokio::test]
    async fn list_queue_initially_empty() {
        let (app, _state) = make_app().await;
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
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_queue_shows_enqueued() {
        let (app, state) = make_app().await;
        let (app, _) = enqueue_entry(app.clone(), &state, "mr-1", Some(50)).await;
        let (app, _) = enqueue_entry(app.clone(), &state, "mr-2", Some(75)).await;

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
        let (app, state) = make_app().await;
        let (app, id) = enqueue_entry(app, &state, "mr-1", None).await;

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
        let (app, _state) = make_app().await;
        let resp = app
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

    /// TASK-095 R3-F2: only a mergeable MR may enter the queue. A
    /// `Reverted` MR enqueued here would be silently RE-MERGED by the
    /// processor — voiding the recovery protocol's Reverted terminal state
    /// and §6's permanent queue removal.
    #[tokio::test]
    async fn enqueue_rejects_non_mergeable_mr() {
        let (app, state) = make_app().await;
        let mut mr = MergeRequest::new(
            Id::new("mr-rev"),
            Id::new("repo-1"),
            Id::new("default"),
            "Reverted MR",
            "feat/rev",
            "main",
            0,
        );
        mr.status = gyre_domain::MrStatus::Reverted;
        state.merge_requests.create(&mr).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "merge_request_id": "mr-rev"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert!(
            state.merge_queue.list_queue().await.unwrap().is_empty(),
            "no queue entry may be created for a non-mergeable MR"
        );
    }
}
