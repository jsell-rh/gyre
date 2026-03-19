pub mod activity;
pub mod agent_messages;
pub mod agents;
pub mod error;
pub mod merge_requests;
pub mod projects;
pub mod repos;
pub mod tasks;
pub mod version;

use axum::{
    routing::{get, post, put},
    Router,
};
use gyre_common::Id;
use std::sync::Arc;

use crate::AppState;

pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/version", get(version::version_handler))
        .route("/api/v1/activity", get(activity::activity_handler))
        // Projects
        .route(
            "/api/v1/projects",
            post(projects::create_project).get(projects::list_projects),
        )
        .route(
            "/api/v1/projects/:id",
            get(projects::get_project)
                .put(projects::update_project)
                .delete(projects::delete_project),
        )
        // Repos
        .route(
            "/api/v1/repos",
            post(repos::create_repo).get(repos::list_repos),
        )
        .route("/api/v1/repos/:id", get(repos::get_repo))
        .route("/api/v1/repos/:id/branches", get(repos::list_branches))
        .route("/api/v1/repos/:id/commits", get(repos::commit_log))
        .route("/api/v1/repos/:id/diff", get(repos::diff))
        // Agents
        .route(
            "/api/v1/agents",
            post(agents::create_agent).get(agents::list_agents),
        )
        .route("/api/v1/agents/:id", get(agents::get_agent))
        .route(
            "/api/v1/agents/:id/status",
            put(agents::update_agent_status),
        )
        .route("/api/v1/agents/:id/heartbeat", put(agents::agent_heartbeat))
        .route(
            "/api/v1/agents/:id/messages",
            get(agent_messages::get_messages).post(agent_messages::send_message),
        )
        // Tasks
        .route(
            "/api/v1/tasks",
            post(tasks::create_task).get(tasks::list_tasks),
        )
        .route(
            "/api/v1/tasks/:id",
            get(tasks::get_task).put(tasks::update_task),
        )
        .route(
            "/api/v1/tasks/:id/status",
            put(tasks::transition_task_status),
        )
        // Merge Requests
        .route(
            "/api/v1/merge-requests",
            post(merge_requests::create_mr).get(merge_requests::list_mrs),
        )
        .route("/api/v1/merge-requests/:id", get(merge_requests::get_mr))
        .route(
            "/api/v1/merge-requests/:id/status",
            put(merge_requests::transition_mr_status),
        )
        .route(
            "/api/v1/merge-requests/:id/comments",
            post(merge_requests::add_comment).get(merge_requests::list_comments),
        )
        .route(
            "/api/v1/merge-requests/:id/reviews",
            post(merge_requests::submit_review).get(merge_requests::list_reviews),
        )
        .route(
            "/api/v1/merge-requests/:id/diff",
            get(merge_requests::get_diff),
        )
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn new_id() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string())
}
