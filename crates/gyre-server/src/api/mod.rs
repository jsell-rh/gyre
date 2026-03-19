pub mod activity;
pub mod admin;
pub mod agent_messages;
pub mod agent_tracking;
pub mod agents;
pub mod auth;
pub mod compose;
pub mod discover;
pub mod error;
pub mod jj;
pub mod merge_queue;
pub mod merge_requests;
pub mod projects;
pub mod repos;
pub mod spawn;
pub mod tasks;
pub mod version;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use compose::{compose_apply, compose_status, compose_teardown};
use discover::{discover_agents, update_agent_card};
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
        // Agent-commit tracking
        .route(
            "/api/v1/repos/:id/commits/record",
            post(agent_tracking::record_commit),
        )
        .route(
            "/api/v1/repos/:id/agent-commits",
            get(agent_tracking::list_commits),
        )
        // Worktree management
        .route(
            "/api/v1/repos/:id/worktrees",
            post(agent_tracking::create_worktree).get(agent_tracking::list_worktrees),
        )
        .route(
            "/api/v1/repos/:id/worktrees/:wt_id",
            delete(agent_tracking::delete_worktree),
        )
        // jj VCS integration
        .route("/api/v1/repos/:id/jj/init", post(jj::jj_init))
        .route("/api/v1/repos/:id/jj/log", get(jj::jj_log))
        .route("/api/v1/repos/:id/jj/new", post(jj::jj_new))
        .route("/api/v1/repos/:id/jj/squash", post(jj::jj_squash))
        .route("/api/v1/repos/:id/jj/undo", post(jj::jj_undo))
        .route("/api/v1/repos/:id/jj/bookmark", post(jj::jj_bookmark))
        // Agents
        .route(
            "/api/v1/agents",
            post(agents::create_agent).get(agents::list_agents),
        )
        .route("/api/v1/agents/spawn", post(spawn::spawn_agent))
        .route("/api/v1/agents/discover", get(discover_agents))
        .route("/api/v1/agents/:id", get(agents::get_agent))
        .route(
            "/api/v1/agents/:id/status",
            put(agents::update_agent_status),
        )
        .route("/api/v1/agents/:id/heartbeat", put(agents::agent_heartbeat))
        .route("/api/v1/agents/:id/complete", post(spawn::complete_agent))
        .route("/api/v1/agents/:id/card", put(update_agent_card))
        .route(
            "/api/v1/agents/:id/messages",
            get(agent_messages::get_messages).post(agent_messages::send_message),
        )
        // Compose
        .route("/api/v1/compose/apply", post(compose_apply))
        .route("/api/v1/compose/status", get(compose_status))
        .route("/api/v1/compose/teardown", post(compose_teardown))
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
        // Merge Queue
        .route("/api/v1/merge-queue/enqueue", post(merge_queue::enqueue))
        .route("/api/v1/merge-queue", get(merge_queue::list_queue))
        .route("/api/v1/merge-queue/:id", delete(merge_queue::cancel_entry))
        // Auth / API keys
        .route("/api/v1/auth/api-keys", post(auth::create_api_key))
        // Admin (Admin role required)
        .route("/api/v1/admin/health", get(admin::admin_health))
        .route("/api/v1/admin/jobs", get(admin::admin_jobs))
        .route("/api/v1/admin/audit", get(admin::admin_audit))
        .route(
            "/api/v1/admin/agents/:id/kill",
            post(admin::admin_kill_agent),
        )
        .route(
            "/api/v1/admin/agents/:id/reassign",
            post(admin::admin_reassign_agent),
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
