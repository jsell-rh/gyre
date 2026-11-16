use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Task, TaskPriority, TaskStatus, TaskType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub parent_task_id: Option<String>,
    pub labels: Option<Vec<String>>,
    /// Task type: "implementation", "delegation", or "coordination". Null for informational tasks.
    pub task_type: Option<String>,
    /// Execution priority (lower = first). Tasks with the same order can run in parallel.
    pub order: Option<u32>,
    /// Task IDs that must complete before this task starts.
    pub depends_on: Option<Vec<String>>,
    /// Spec path this task implements (e.g. "system/hello-world.md"). Bare path.
    pub spec_path: Option<String>,
    /// Pinned approved spec blob this task implements: "path@sha". Optional;
    /// when absent, decomposition falls back to the ledger's current SHA.
    pub spec_ref: Option<String>,
    /// Workspace ID to scope this task to.
    pub workspace_id: Option<String>,
    /// Repository ID this task targets.
    pub repo_id: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub assigned_to: Option<String>,
    pub branch: Option<String>,
    pub pr_link: Option<String>,
    pub labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct TransitionStatusRequest {
    pub status: String,
}

#[derive(Deserialize)]
pub struct ListTasksQuery {
    pub status: Option<String>,
    pub assigned_to: Option<String>,
    pub parent_task_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub assigned_to: Option<String>,
    pub parent_task_id: Option<String>,
    pub labels: Vec<String>,
    pub branch: Option<String>,
    pub pr_link: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    pub workspace_id: String,
    pub repo_id: String,
    /// Deployment generation — incremented on reassignment (§2.4 replay prevention).
    pub generation: u32,
}

impl From<Task> for TaskResponse {
    fn from(t: Task) -> Self {
        Self {
            id: t.id.to_string(),
            title: t.title,
            description: t.description,
            status: task_status_str(&t.status),
            priority: task_priority_str(&t.priority),
            assigned_to: t.assigned_to.map(|id| id.to_string()),
            parent_task_id: t.parent_task_id.map(|id| id.to_string()),
            labels: t.labels,
            branch: t.branch,
            pr_link: t.pr_link,
            created_at: t.created_at,
            updated_at: t.updated_at,
            spec_path: t.spec_path,
            spec_ref: t.spec_ref,
            task_type: t.task_type.map(|tt| task_type_str(&tt)),
            order: t.order,
            depends_on: t.depends_on.into_iter().map(|id| id.to_string()).collect(),
            workspace_id: t.workspace_id.to_string(),
            repo_id: t.repo_id.to_string(),
            generation: t.generation,
        }
    }
}

fn task_status_str(s: &TaskStatus) -> String {
    match s {
        TaskStatus::Backlog => "backlog",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Review => "review",
        TaskStatus::Done => "done",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Cancelled => "cancelled",
    }
    .to_string()
}

fn parse_task_status(s: &str) -> Result<TaskStatus, ApiError> {
    match s.to_lowercase().as_str() {
        "backlog" => Ok(TaskStatus::Backlog),
        "in_progress" | "inprogress" => Ok(TaskStatus::InProgress),
        "review" => Ok(TaskStatus::Review),
        "done" => Ok(TaskStatus::Done),
        "blocked" => Ok(TaskStatus::Blocked),
        "cancelled" | "canceled" => Ok(TaskStatus::Cancelled),
        _ => Err(ApiError::InvalidInput(format!("unknown task status: {s}"))),
    }
}

fn task_type_str(tt: &TaskType) -> String {
    match tt {
        TaskType::Implementation => "implementation",
        TaskType::Delegation => "delegation",
        TaskType::Coordination => "coordination",
    }
    .to_string()
}

fn parse_task_type(s: &str) -> Result<TaskType, ApiError> {
    match s.to_lowercase().as_str() {
        "implementation" => Ok(TaskType::Implementation),
        "delegation" => Ok(TaskType::Delegation),
        "coordination" => Ok(TaskType::Coordination),
        _ => Err(ApiError::InvalidInput(format!("unknown task type: {s}"))),
    }
}

fn task_priority_str(p: &TaskPriority) -> String {
    match p {
        TaskPriority::Low => "low",
        TaskPriority::Medium => "medium",
        TaskPriority::High => "high",
        TaskPriority::Critical => "critical",
    }
    .to_string()
}

fn parse_task_priority(s: &str) -> Result<TaskPriority, ApiError> {
    match s.to_lowercase().as_str() {
        "low" => Ok(TaskPriority::Low),
        "medium" => Ok(TaskPriority::Medium),
        "high" => Ok(TaskPriority::High),
        "critical" => Ok(TaskPriority::Critical),
        _ => Err(ApiError::InvalidInput(format!("unknown priority: {s}"))),
    }
}

pub async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    let now = now_secs();
    let mut task = Task::new(new_id(), req.title, now);
    task.description = req.description;
    if let Some(p) = req.priority {
        task.priority = parse_task_priority(&p)?;
    }
    task.parent_task_id = req.parent_task_id.map(Id::new);
    task.labels = req.labels.unwrap_or_default();
    if let Some(tt) = req.task_type {
        task.task_type = Some(parse_task_type(&tt)?);
    }
    task.order = req.order;
    task.depends_on = req
        .depends_on
        .unwrap_or_default()
        .into_iter()
        .map(Id::new)
        .collect();
    task.spec_path = req.spec_path;
    task.spec_ref = req.spec_ref;
    if let Some(ws_id) = req.workspace_id {
        task.workspace_id = Id::new(ws_id);
    }
    if let Some(repo_id) = req.repo_id {
        task.repo_id = Id::new(repo_id);
    }
    state.tasks.create(&task).await?;
    // Index for search (search.md §Searchable Entities: task facets
    // status/priority/assigned_to, workspace/repo scope).
    crate::search_indexer::index(&state, crate::search_indexer::task_document(&task)).await;
    state
        .emit_event(
            Some(task.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
            gyre_common::message::MessageKind::TaskCreated,
            Some(serde_json::json!({"task_id": task.id.to_string()})),
        )
        .await;
    Ok((StatusCode::CREATED, Json(TaskResponse::from(task))))
}

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let tasks = if let Some(repo_id) = params.repo_id {
        state.tasks.list_by_repo(&Id::new(repo_id)).await?
    } else if let Some(ws_id) = params.workspace_id {
        state.tasks.list_by_workspace(&Id::new(ws_id)).await?
    } else {
        match (params.status, params.assigned_to, params.parent_task_id) {
            (Some(status_str), _, _) => {
                let status = parse_task_status(&status_str)?;
                state.tasks.list_by_status(&status).await?
            }
            (_, Some(agent_id), _) => state.tasks.list_by_assignee(&Id::new(agent_id)).await?,
            (_, _, Some(parent_id)) => state.tasks.list_by_parent(&Id::new(parent_id)).await?,
            _ => state.tasks.list().await?,
        }
    };
    Ok(Json(tasks.into_iter().map(TaskResponse::from).collect()))
}

/// GET /api/v1/workspaces/:workspace_id/tasks — list tasks scoped to a workspace.
/// Primary access pattern per api-conventions.md §1.1.
pub async fn list_workspace_tasks(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let tasks = state
        .tasks
        .list_by_workspace(&Id::new(workspace_id))
        .await?;
    Ok(Json(tasks.into_iter().map(TaskResponse::from).collect()))
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let task = state
        .tasks
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {id} not found")))?;
    Ok(Json(TaskResponse::from(task)))
}

pub async fn update_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {id} not found")))?;
    if let Some(title) = req.title {
        task.title = title;
    }
    if let Some(desc) = req.description {
        task.description = Some(desc);
    }
    if let Some(p) = req.priority {
        task.priority = parse_task_priority(&p)?;
    }
    if let Some(agent_id) = req.assigned_to {
        let new_assignee = Id::new(agent_id);
        // §2.4 replay prevention: reassignment bumps the deployment generation,
        // invalidating SignedInputs pinned to the previous generation.
        if task.assigned_to.as_ref() != Some(&new_assignee) {
            task.generation = task.generation.saturating_add(1);
        }
        task.assigned_to = Some(new_assignee);
    }
    if let Some(branch) = req.branch {
        task.branch = Some(branch);
    }
    if let Some(pr) = req.pr_link {
        task.pr_link = Some(pr);
    }
    if let Some(labels) = req.labels {
        task.labels = labels;
    }
    task.updated_at = now_secs();
    state.tasks.update(&task).await?;
    // search.md §Index Updates: task updated -> index updated (status/priority
    // facets must reflect current values, not create-time ones).
    crate::search_indexer::index(&state, crate::search_indexer::task_document(&task)).await;
    Ok(Json(TaskResponse::from(task)))
}

#[instrument(skip(state, req), fields(task_id = %id, new_status = %req.status))]
pub async fn transition_task_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<TransitionStatusRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let mut task = state
        .tasks
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("task {id} not found")))?;
    let new_status = parse_task_status(&req.status)?;
    let old_status = task.status.clone();
    task.transition_status(new_status.clone())
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    let ts = now_secs();
    task.updated_at = ts;
    state.tasks.update(&task).await?;
    // search.md §Index Updates: status change -> index updated.
    crate::search_indexer::index(&state, crate::search_indexer::task_document(&task)).await;
    state
        .emit_event(
            Some(task.workspace_id.clone()),
            gyre_common::message::Destination::Workspace(task.workspace_id.clone()),
            gyre_common::message::MessageKind::TaskTransitioned,
            Some(serde_json::json!({"task_id": task.id.to_string(), "status": req.status})),
        )
        .await;

    // meta-spec-reconciliation.md §11: ReconciliationCompleted /
    // MetaSpecDriftResolved fire when the last open reconciliation /
    // drift-review task for a workspace reaches terminal status. The
    // shared hook also advances a §7 rolling rollout (next batch) before
    // the completion check.
    if matches!(
        task.status,
        gyre_domain::TaskStatus::Done | gyre_domain::TaskStatus::Cancelled
    ) && task.labels.iter().any(|l| {
        l == crate::reconciliation::RECONCILIATION_LABEL
            || l == crate::reconciliation::DRIFT_REVIEW_LABEL
    }) {
        crate::reconciliation::on_reconciliation_task_terminal(&state, &task.workspace_id).await;
    }

    // Auto-track status transition as analytics event (task-146 helper:
    // spec-required old_status/new_status/assigned_to properties,
    // caller-bound scope, search-indexed on write).
    super::analytics::record_task_status_changed(&state, &task, &old_status).await;

    Ok(Json(TaskResponse::from(task)))
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

    async fn create_test_task(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({ "title": title });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
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
    async fn create_task_defaults() {
        let (_, id) = create_test_task(app(), "My Task").await;
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn get_task_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks/no-such")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn task_crud_flow() {
        let app = app();
        let (app, id) = create_test_task(app, "Build API").await;

        // Get
        let get_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/tasks/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let got = body_json(get_resp).await;
        assert_eq!(got["status"], "backlog");
        assert_eq!(got["priority"], "medium");

        // Update
        let update_body = serde_json::json!({ "priority": "high" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["priority"], "high");
    }

    #[tokio::test]
    async fn task_status_transition_valid() {
        let app = app();
        let (app, id) = create_test_task(app, "Trans Task").await;

        let body = serde_json::json!({ "status": "in_progress" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "in_progress");
    }

    #[tokio::test]
    async fn task_status_transition_invalid() {
        let app = app();
        let (app, id) = create_test_task(app, "Bad Trans").await;

        // Backlog -> Done is invalid
        let body = serde_json::json!({ "status": "done" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Regression (task-202 review): the `task.status_changed` activity event
    /// emitted on the transition write path must carry its workspace/repo
    /// scope, so search.md §Searchable Entities' Activity Event workspace/repo
    /// filters actually match write-path-indexed events (the scope used to be
    /// dropped at every emit site except the orchestrator spawn path).
    #[tokio::test]
    async fn task_transition_activity_event_is_workspace_scoped() {
        use gyre_ports::search::SearchQuery;
        use std::sync::Arc;

        // Real SQLite FTS5 backend — the workspace filter is SQL, not a stub.
        let db = tempfile::NamedTempFile::new().unwrap();
        let storage = gyre_adapters::SqliteStorage::new(db.path().to_str().unwrap()).unwrap();
        let state = crate::mem::test_state_with_search(
            Arc::new(storage) as Arc<dyn gyre_ports::SearchPort>
        );
        let app = crate::api::api_router().with_state(state.clone());

        // Create a task scoped to a known workspace/repo.
        let body = serde_json::json!({
            "title": "Scoped transition probe",
            "workspace_id": "ws-scope-1",
            "repo_id": "repo-scope-1",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();

        // Transition it — emits and indexes task.status_changed.
        let body = serde_json::json!({ "status": "in_progress" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The activity event must be retrievable through the workspace
        // filter — this is exactly the query that returned nothing when the
        // emit sites dropped the scope.
        let results = state
            .search
            .search(SearchQuery {
                query: "task.status_changed".to_string(),
                parsed: gyre_ports::search::parse_query("task.status_changed"),
                entity_type: Some("activity_event".to_string()),
                workspace_id: Some("ws-scope-1".to_string()),
                limit: 10,
                tenant_id: String::new(),
                accessible_workspace_ids: None,
                accessible_repo_ids: None,
            })
            .await
            .unwrap();
        assert_eq!(
            results.len(),
            1,
            "activity event must be findable via the workspace filter"
        );
        assert_eq!(
            results[0].facets.get("workspace").map(String::as_str),
            Some("ws-scope-1")
        );
        assert_eq!(
            results[0].facets.get("repo").map(String::as_str),
            Some("repo-scope-1")
        );
    }

    #[tokio::test]
    async fn list_tasks_by_status() {
        let app = app();
        let (app, id) = create_test_task(app, "Filter Task").await;
        let body = serde_json::json!({ "status": "in_progress" });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks?status=in_progress")
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
    async fn task_status_transition_emits_analytics_event() {
        // analytics.md §Auto-Emitted Events: task.status_changed must carry
        // old_status, new_status, task_id, assigned_to.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let (app, id) = create_test_task(app, "Analytics task").await;

        // Assign the task so assigned_to is populated in the event.
        let assign_body = serde_json::json!({ "assigned_to": "agent-77" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&assign_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = serde_json::json!({ "status": "in_progress" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = state
            .analytics
            .query(Some("task.status_changed"), None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "one status_changed event expected");
        let ev = &events[0];
        assert_eq!(ev.properties["task_id"], id);
        assert_eq!(ev.properties["old_status"], "Backlog");
        assert_eq!(ev.properties["new_status"], "InProgress");
        assert_eq!(ev.properties["assigned_to"], "agent-77");
        // agent_id on the event is the assigned agent.
        assert_eq!(ev.agent_id.as_deref(), Some("agent-77"));
    }
}
