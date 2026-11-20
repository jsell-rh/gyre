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

/// POST /api/v1/merge-queue/enqueue
///
/// identity-security.md §Task-Scoped JWT Claims — merge-queue boundary
/// (task-263 review repair): an agent principal may only enqueue an MR
/// inside its task scope — the MR's repository must be the repo of the
/// caller's `task_id` claim, not merely a repo its tier can reach.
/// Enqueuing a foreign task's MR would push that task's work through the
/// merge pipeline (gate execution, then the system merge that
/// transitions the task) on the caller's request. The Ralph loop's
/// automatic post-completion enqueue goes through `enqueue_core` (system
/// actor) and is unaffected. Non-agent principals are unaffected.
#[instrument(skip(state, auth, req), fields(mr_id = %req.merge_request_id))]
pub async fn enqueue(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<EnqueueRequest>,
) -> Result<(StatusCode, Json<QueueEntryResponse>), ApiError> {
    // Agent task-scope boundary (see handler doc): resolve the target MR
    // and require its repo to be within the caller's task claim.
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(&req.merge_request_id))
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("merge request {} not found", req.merge_request_id))
        })?;
    super::merge_requests::require_repo_in_agent_task_scope(
        &state,
        &auth,
        &mr.repository_id.to_string(),
        "enqueue this merge request",
    )
    .await?;
    // Preview agents (meta-spec reconciliation §5) skip the merge queue like
    // they skip MRs — the preview branch is the deliverable. Lower reachability
    // than the task guards (enqueue needs an existing MR), but the same
    // Agent-role 'write' escape applies, so guard it here too.
    if crate::api::meta_specs::caller_is_preview_agent(&state, &auth).await {
        return Err(ApiError::Forbidden(
            "preview agents may not enqueue merge requests — preview runs skip merge-queue ceremony"
                .to_string(),
        ));
    }
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

    // Broadcast-only QueueUpdated (message-bus.md §Broadcast-only kinds):
    // the enqueue transition — an MR entering the merge queue. Storage-exempt
    // broadcast-channel delivery only.
    crate::merge_processor::emit_queue_updated(state, &mr.workspace_id).await;

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

/// DELETE /api/v1/merge-queue/:id — cancel a queue entry.
///
/// identity-security.md §Task-Scoped JWT Claims — merge-queue boundary
/// (task-263 review repair): an agent principal may only cancel a queue
/// entry whose MR is inside its task scope — the mirror of the enqueue
/// boundary above (an agent must not be able to dequeue another task's
/// MR either). The in-handler effect is queue-row mutation only (no task
/// rows), but cancelling a foreign task's entry disrupts that task's
/// delivery path. Non-agent principals are unaffected.
pub async fn cancel_entry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    // Preview agents may not cancel merge-queue entries (meta-spec
    // reconciliation §5 skip-ceremony; same escape class as the enqueue
    // guard above) — cancelling another run's entry mutates the merge
    // pipeline from throwaway preview scratch work.
    if crate::api::meta_specs::caller_is_preview_agent(&state, &auth).await {
        return Err(ApiError::Forbidden(
            "preview agents may not cancel merge queue entries — preview runs skip \
             merge-queue ceremony"
                .to_string(),
        ));
    }
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

    // Agent task-scope boundary (see handler doc): resolve the entry's MR
    // and require its repo to be within the caller's task claim. The MR
    // load doubles as the workspace source for the QueueUpdated broadcast
    // below; for an agent principal an unresolvable MR fails the boundary
    // closed, while non-agent principals keep the pre-existing cleanup
    // path (cancel an orphaned entry, skip the broadcast).
    let mr = state
        .merge_requests
        .find_by_id(&entry.merge_request_id)
        .await?;
    if auth.agent_task_claim().is_some() {
        let mr = mr.as_ref().ok_or_else(|| {
            ApiError::NotFound(format!(
                "merge request {} not found",
                entry.merge_request_id
            ))
        })?;
        super::merge_requests::require_repo_in_agent_task_scope(
            &state,
            &auth,
            &mr.repository_id.to_string(),
            "cancel this merge-queue entry",
        )
        .await?;
    }

    state.merge_queue.cancel(&Id::new(&id)).await?;

    // Broadcast-only QueueUpdated (message-bus.md §Broadcast-only kinds):
    // the cancel transition — the MR leaving the merge queue. The workspace
    // is resolved from the entry's MR; if the MR no longer resolves the
    // emission is skipped (the queue mutation itself already succeeded).
    if let Some(mr) = mr {
        crate::merge_processor::emit_queue_updated(&state, &mr.workspace_id).await;
    }

    // merge_queue.processed analytics event (analytics.md §Auto-Emitted
    // Events: "Queue entry processed") — an operator cancellation is a
    // terminal outcome of the entry, same as the circuit-breaker Cancelled
    // path; the spec outcome value is "skipped".
    crate::merge_processor::emit_queue_processed_terminal(
        &state,
        &entry,
        None,
        "skipped",
        "cancelled",
        "cancelled by operator",
    )
    .await;

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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// message-bus.md §Broadcast-only kinds: a merge-queue state transition
    /// (an MR entering the queue, then leaving it via operator cancel) must
    /// emit a server-originated `QueueUpdated` broadcast observable by a
    /// subscribed client — `Destination::Broadcast`, `workspace_id: None`,
    /// payload none — and the kind's storage exemption means no row is ever
    /// persisted. Before task-249 the kind had zero production emitters, so
    /// this assertion could never pass.
    #[tokio::test]
    async fn enqueue_and_cancel_broadcast_queue_updated() {
        use gyre_common::message::{Destination, MessageKind, MessageOrigin};

        let (app, state) = make_app().await;
        // The QueueUpdated emitter resolves the tenant from the MR's
        // workspace record (skip-and-log when unresolvable), so the seeded
        // MR's workspace must exist for the broadcast to fire.
        crate::mem::ensure_test_workspace(&state, "default").await;
        seed_mr(&state, "mr-qu").await;
        let mut rx = state.message_broadcast_tx.subscribe();

        // Drain helper: the channel is shared with other emissions (e.g.
        // gate triggers), so scan for the QueueUpdated broadcast. try_recv is
        // synchronous — the handler sends before the HTTP response returns.
        fn next_queue_updated(
            rx: &mut tokio::sync::broadcast::Receiver<gyre_common::message::Message>,
        ) -> Option<gyre_common::message::Message> {
            for _ in 0..32 {
                match rx.try_recv() {
                    Ok(m) if m.kind == MessageKind::QueueUpdated => return Some(m),
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
            None
        }

        // Enqueue: the MR entering the queue.
        let body = serde_json::json!({ "merge_request_id": "mr-qu" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-queue/enqueue")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let entry_id = json["id"].as_str().unwrap().to_string();

        let msg = next_queue_updated(&mut rx)
            .expect("enqueue transition must broadcast QueueUpdated to subscribers");
        assert!(matches!(msg.from, MessageOrigin::Server));
        assert_eq!(msg.to, Destination::Broadcast);
        assert_eq!(
            msg.workspace_id, None,
            "broadcast-only kinds carry no workspace scope"
        );
        assert!(
            msg.payload.is_none(),
            "QueueUpdated payload is (none) per spec"
        );
        assert!(
            state.messages.find_by_id(&msg.id).await.unwrap().is_none(),
            "QueueUpdated is storage-exempt: it must never hit MessageRepository::store()"
        );

        // Cancel: the MR leaving the queue.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/merge-queue/{entry_id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let msg = next_queue_updated(&mut rx)
            .expect("cancel transition must broadcast QueueUpdated to subscribers");
        assert!(matches!(msg.from, MessageOrigin::Server));
        assert_eq!(msg.to, Destination::Broadcast);
        assert_eq!(msg.workspace_id, None);
        assert!(
            state.messages.find_by_id(&msg.id).await.unwrap().is_none(),
            "QueueUpdated is storage-exempt: it must never hit MessageRepository::store()"
        );
    }

    /// analytics.md §Auto-Emitted Events: `merge_queue.processed` (trigger
    /// "Queue entry processed") must record on every terminal outcome. An
    /// operator cancellation terminates the entry, same as the
    /// circuit-breaker Cancelled path — outcome "skipped". Regression:
    /// this path previously cancelled the entry with no analytics event.
    #[tokio::test]
    async fn cancel_records_queue_processed_skipped() {
        let (app, state) = make_app().await;
        let (app, id) = enqueue_entry(app, &state, "mr-1", None).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/merge-queue/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let events = state
            .analytics
            .query(Some("merge_queue.processed"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            events.len(),
            1,
            "operator cancellation must record merge_queue.processed"
        );
        let ev = &events[0];
        assert_eq!(ev.properties["mr_id"], "mr-1");
        assert_eq!(ev.properties["outcome"], "skipped");
        assert_eq!(ev.properties["failure_kind"], "cancelled");
        assert!(
            ev.properties["wait_secs"].as_u64().is_some(),
            "wait_secs (spec-required) must be present"
        );
    }

    #[tokio::test]
    async fn cancel_not_found() {
        let (app, _state) = make_app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/merge-queue/no-such")
                    // task-263: cancel_entry takes an auth extractor (agent
                    // task-scope boundary) — the not-found path is probed by
                    // an authenticated caller; an unauthenticated request is
                    // (correctly) rejected 401 before the entry lookup.
                    .header("authorization", "Bearer test-token")
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
                    .header("authorization", "Bearer test-token")
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

    /// task-263 review repair (identity-security.md §Task-Scoped JWT
    /// Claims, merge-queue boundary): a valid, non-expired worker JWT
    /// spawned for a task in repo-9 gets 403 on enqueueing another task's
    /// MR (repo-1) and on cancelling that MR's queue entry — the entry
    /// stays queued — while a worker scoped to repo-1 keeps both rights
    /// on the same fixture (the boundary is the task claim, not the
    /// route). The Ralph loop's automatic enqueue goes through
    /// `enqueue_core` (system actor) and never crosses this boundary.
    #[tokio::test]
    async fn agent_jwt_task_scope_boundary_on_merge_queue_surfaces() {
        let (app, state) = make_app().await;
        // The target MR lives in repo-1 (seed_mr's fixture repo).
        seed_mr(&state, "mr-mq-scope").await;
        // Task rows the two tokens' claims resolve to: one in repo-1 (the
        // MR's repo), one in repo-9 (foreign).
        for (task_id, repo_id) in [("task-mq-own", "repo-1"), ("task-mq-foreign", "repo-9")] {
            let mut task = gyre_domain::Task::new(Id::new(task_id), "merge-queue scope task", 1000);
            task.workspace_id = Id::new("default");
            task.repo_id = Id::new(repo_id);
            state.tasks.create(&task).await.unwrap();
        }
        let mint = |agent_id: &str, task_id: &str, repo_id: &str| {
            state
                .agent_signing_key
                .mint_worker_scoped(
                    agent_id,
                    task_id,
                    "system",
                    &state.base_url,
                    3600,
                    "default",
                    "default",
                    repo_id,
                    repo_id,
                    None,
                    Some(1),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
        };
        let token_own = mint("agent-mq-own", "task-mq-own", "repo-1");
        let token_foreign = mint("agent-mq-foreign", "task-mq-foreign", "repo-9");
        state
            .kv_store
            .kv_set("agent_tokens", "agent-mq-own", token_own.clone())
            .await
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-mq-foreign", token_foreign.clone())
            .await
            .unwrap();

        async fn enqueue_as(app: &Router, token: &str) -> axum::response::Response {
            let body = serde_json::to_vec(&serde_json::json!({
                "merge_request_id": "mr-mq-scope"
            }))
            .unwrap();
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/merge-queue/enqueue")
                        .header("content-type", "application/json")
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
        }
        async fn cancel_as(app: &Router, entry_id: &str, token: &str) -> axum::response::Response {
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!("/api/v1/merge-queue/{entry_id}"))
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        }

        // Foreign task's token: enqueue → 403, no queue entry created.
        let resp = enqueue_as(&app, &token_foreign).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(
            state.merge_queue.list_queue().await.unwrap().is_empty(),
            "no queue entry may be created for a foreign task's MR"
        );

        // Own-repo token: enqueue → 201 (positive control).
        let resp = enqueue_as(&app, &token_own).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let entry_id = body_json(resp).await["id"].as_str().unwrap().to_string();

        // Foreign task's token: cancel → 403, the entry stays queued.
        let resp = cancel_as(&app, &entry_id, &token_foreign).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let entry = state
            .merge_queue
            .find_by_id(&Id::new(&entry_id))
            .await
            .unwrap()
            .expect("queue entry must survive the denied cancel");
        assert_eq!(entry.status, gyre_domain::MergeQueueEntryStatus::Queued);

        // Own-repo token: cancel → 204 (positive control).
        let resp = cancel_as(&app, &entry_id, &token_own).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
