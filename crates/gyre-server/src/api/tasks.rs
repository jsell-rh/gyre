use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{Task, TaskPriority, TaskStatus};
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
        _ => Err(ApiError::InvalidInput(format!("unknown task status: {s}"))),
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
    state.tasks.create(&task).await?;
    Ok((StatusCode::CREATED, Json(TaskResponse::from(task))))
}

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let tasks = match (params.status, params.assigned_to, params.parent_task_id) {
        (Some(status_str), _, _) => {
            let status = parse_task_status(&status_str)?;
            state.tasks.list_by_status(&status).await?
        }
        (_, Some(agent_id), _) => state.tasks.list_by_assignee(&Id::new(agent_id)).await?,
        (_, _, Some(parent_id)) => state.tasks.list_by_parent(&Id::new(parent_id)).await?,
        _ => state.tasks.list().await?,
    };
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
        task.assigned_to = Some(Id::new(agent_id));
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
    task.transition_status(new_status)
        .map_err(|e| ApiError::InvalidInput(e.to_string()))?;
    task.updated_at = now_secs();
    state.tasks.update(&task).await?;
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
}
