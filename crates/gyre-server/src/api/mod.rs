pub mod activity;
pub mod admin;
pub mod agent_logs;
pub mod agent_messages;
pub mod agent_tracking;
pub mod agents;
pub mod analytics;
pub mod audit;
pub mod auth;
pub mod code_awareness;
pub mod compose;
pub mod compute;
pub mod discover;
pub mod error;
pub mod gates;
pub mod jj;
pub mod merge_queue;
pub mod merge_requests;
pub mod network;
pub mod projects;
pub mod provenance;
pub mod push_gates;
pub mod repos;
pub mod spawn;
pub mod speculative;
pub mod tasks;
pub mod version;

use audit::{
    audit_stats, audit_stream, create_siem_target, delete_siem_target, list_siem_targets,
    query_audit_events, record_audit_event, update_siem_target,
};
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use compose::{compose_apply, compose_status, compose_teardown};
use compute::{
    create_compute_target, delete_compute_target, get_compute_target, list_compute_targets,
};
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
        .route("/api/v1/repos/mirror", post(repos::create_mirror_repo))
        .route("/api/v1/repos/:id", get(repos::get_repo))
        .route("/api/v1/repos/:id/branches", get(repos::list_branches))
        .route("/api/v1/repos/:id/commits", get(repos::commit_log))
        .route("/api/v1/repos/:id/diff", get(repos::diff))
        .route("/api/v1/repos/:id/mirror/sync", post(repos::sync_mirror))
        // Commit provenance (M13.2)
        .route(
            "/api/v1/repos/:id/provenance",
            get(provenance::get_provenance),
        )
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
        // Quality gates
        .route(
            "/api/v1/repos/:id/gates",
            post(gates::create_gate).get(gates::list_gates),
        )
        .route(
            "/api/v1/repos/:id/gates/:gate_id",
            delete(gates::delete_gate),
        )
        // Pre-accept push gates
        .route(
            "/api/v1/repos/:id/push-gates",
            get(push_gates::get_push_gates).put(push_gates::set_push_gates),
        )
        // Cross-agent code awareness (M13.4)
        .route("/api/v1/repos/:id/blame", get(code_awareness::get_blame))
        .route(
            "/api/v1/repos/:id/hot-files",
            get(code_awareness::get_hot_files),
        )
        .route(
            "/api/v1/repos/:id/review-routing",
            get(code_awareness::get_review_routing),
        )
        // Speculative merging (M13.5)
        .route(
            "/api/v1/repos/:id/speculative",
            get(speculative::list_speculative),
        )
        .route(
            "/api/v1/repos/:id/speculative/:branch",
            get(speculative::get_speculative_branch),
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
        // Agent touched paths (M13.4)
        .route(
            "/api/v1/agents/:id/touched-paths",
            get(code_awareness::get_touched_paths),
        )
        // Agent logs
        .route(
            "/api/v1/agents/:id/logs",
            post(agent_logs::append_log).get(agent_logs::get_logs),
        )
        .route(
            "/api/v1/agents/:id/logs/stream",
            get(agent_logs::stream_logs),
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
        .route(
            "/api/v1/merge-requests/:id/gates",
            get(gates::list_mr_gate_results),
        )
        // Merge Queue
        .route("/api/v1/merge-queue/enqueue", post(merge_queue::enqueue))
        .route("/api/v1/merge-queue", get(merge_queue::list_queue))
        .route("/api/v1/merge-queue/:id", delete(merge_queue::cancel_entry))
        // Auth / API keys
        .route("/api/v1/auth/api-keys", post(auth::create_api_key))
        // Analytics
        .route(
            "/api/v1/analytics/events",
            post(analytics::record_event).get(analytics::query_events),
        )
        .route("/api/v1/analytics/count", get(analytics::count_events))
        .route("/api/v1/analytics/daily", get(analytics::daily_events))
        // Costs
        .route(
            "/api/v1/costs",
            post(analytics::record_cost).get(analytics::query_costs),
        )
        .route("/api/v1/costs/summary", get(analytics::cost_summary))
        // Audit events
        .route(
            "/api/v1/audit/events",
            post(record_audit_event).get(query_audit_events),
        )
        .route("/api/v1/audit/stream", get(audit_stream))
        .route("/api/v1/audit/stats", get(audit_stats))
        // Admin (Admin role required)
        .route("/api/v1/admin/health", get(admin::admin_health))
        .route("/api/v1/admin/jobs", get(admin::admin_jobs))
        .route("/api/v1/admin/jobs/:name/run", post(admin::admin_run_job))
        .route("/api/v1/admin/audit", get(admin::admin_audit))
        .route(
            "/api/v1/admin/agents/:id/kill",
            post(admin::admin_kill_agent),
        )
        .route(
            "/api/v1/admin/agents/:id/reassign",
            post(admin::admin_reassign_agent),
        )
        // Snapshot / Restore
        .route("/api/v1/admin/snapshot", post(admin::admin_create_snapshot))
        .route("/api/v1/admin/snapshots", get(admin::admin_list_snapshots))
        .route("/api/v1/admin/restore", post(admin::admin_restore_snapshot))
        .route(
            "/api/v1/admin/snapshots/:id",
            delete(admin::admin_delete_snapshot),
        )
        // Seed data
        .route("/api/v1/admin/seed", post(admin::admin_seed))
        // Data Export
        .route("/api/v1/admin/export", get(admin::admin_export))
        // Retention Policies
        .route(
            "/api/v1/admin/retention",
            get(admin::admin_list_retention).put(admin::admin_update_retention),
        )
        // SIEM targets
        .route(
            "/api/v1/admin/siem",
            post(create_siem_target).get(list_siem_targets),
        )
        .route(
            "/api/v1/admin/siem/:id",
            put(update_siem_target).delete(delete_siem_target),
        )
        // Compute Targets
        .route(
            "/api/v1/admin/compute-targets",
            post(create_compute_target).get(list_compute_targets),
        )
        .route(
            "/api/v1/admin/compute-targets/:id",
            get(get_compute_target).delete(delete_compute_target),
        )
        // Network peers (WireGuard mesh)
        .route(
            "/api/v1/network/peers",
            post(network::register_peer).get(network::list_peers),
        )
        .route(
            "/api/v1/network/peers/agent/:agent_id",
            get(network::get_peer_by_agent),
        )
        .route("/api/v1/network/peers/:id", delete(network::delete_peer))
        .route("/api/v1/network/derp-map", get(network::derp_map))
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
