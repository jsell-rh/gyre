pub mod activity;
pub mod admin;
pub mod agent_logs;
pub mod agent_messages;
pub mod agent_tracking;
pub mod agents;
pub mod aibom;
pub mod analytics;
pub mod audit;
pub mod auth;
pub mod budget;
pub mod code_awareness;
pub mod compose;
pub mod compute;
pub mod compute_targets;
pub mod container;
pub mod conversations;
pub mod dependencies;
pub mod discover;
pub mod error;
pub mod explorer_views;
pub mod federation;
pub mod gates;
pub mod graph;
pub mod jj;
pub mod key_binding;
pub mod llm_config;
pub mod llm_prompts;
pub mod merge_deps;
pub mod merge_queue;
pub mod merge_requests;
pub mod messages;
pub mod meta_specs;
pub mod mr_timeline;
pub mod network;
pub mod personas;
pub mod policies;
pub mod provenance;
pub mod push_gates;
pub mod release;
pub mod repos;
pub mod saved_views;
pub mod scim;
pub mod search;
pub mod spawn;
pub mod spec_assertions;
pub mod spec_policy;
pub mod specs;
pub mod specs_assist;
pub mod speculative;
pub mod stack_attest;
pub mod tasks;
pub mod tenants;
pub mod traces;
pub mod trust_anchors;
pub mod users;
pub mod version;
pub mod workload;
pub mod workspaces;

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
    close_tunnel, create_compute_target, delete_compute_target, get_compute_target,
    list_compute_targets, list_tunnels, open_tunnel,
};
use discover::{discover_agents, get_agent_card, update_agent_card};
use gyre_common::Id;
use std::sync::Arc;
use users::{
    create_team, create_token, delete_team, delete_token, dismiss_notification, get_judgments,
    get_me, get_my_agents, get_my_mrs, get_my_notifications, get_my_tasks, get_notification_count,
    get_notification_preferences, invite_member, list_members, list_teams, list_tokens,
    remove_member, resolve_notification, update_me, update_member_role,
    update_notification_preferences, update_team,
};

use crate::AppState;

pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/version", get(version::version_handler))
        .route("/api/v1/activity", get(activity::activity_handler))
        // Repos
        .route(
            "/api/v1/repos",
            post(repos::create_repo).get(repos::list_repos),
        )
        .route("/api/v1/repos/mirror", post(repos::create_mirror_repo))
        .route(
            "/api/v1/repos/:id",
            get(repos::get_repo)
                .put(repos::update_repo)
                .delete(repos::delete_repo),
        )
        .route("/api/v1/repos/:id/archive", post(repos::archive_repo))
        .route("/api/v1/repos/:id/unarchive", post(repos::unarchive_repo))
        .route("/api/v1/repos/:id/branches", get(repos::list_branches))
        .route("/api/v1/repos/:id/commits", get(repos::commit_log))
        .route("/api/v1/repos/:id/diff", get(repos::diff))
        .route("/api/v1/repos/:id/mirror/sync", post(repos::sync_mirror))
        // Commit provenance (M13.2)
        .route(
            "/api/v1/repos/:id/provenance",
            get(provenance::get_provenance),
        )
        // Attestation verification (TASK-008, §6.4)
        .route(
            "/api/v1/repos/:id/attestations/:commit_sha/verification",
            get(provenance::get_verification),
        )
        // Attestation bundle export (TASK-008, §6.3)
        .route(
            "/api/v1/repos/:id/attestations/:commit_sha/bundle",
            get(provenance::get_attestation_bundle),
        )
        // Attestation chain visualization (TASK-009, §7.6)
        .route(
            "/api/v1/repos/:id/attestations/:commit_sha/chain",
            get(provenance::get_attestation_chain),
        )
        // AI Bill of Materials (M14.3)
        .route("/api/v1/repos/:id/aibom", get(aibom::get_aibom))
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
            "/api/v1/repos/:id/worktrees/:worktree_id",
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
        // Stack attestation policy (M14.2)
        .route(
            "/api/v1/repos/:id/stack-policy",
            get(stack_attest::get_stack_policy).put(stack_attest::set_stack_policy),
        )
        // Spec enforcement policy (M12.3)
        .route(
            "/api/v1/repos/:id/spec-policy",
            get(spec_policy::get_spec_policy).put(spec_policy::set_spec_policy),
        )
        // ABAC policies (G6)
        .route(
            "/api/v1/repos/:id/abac-policy",
            get(crate::abac::get_abac_policy).put(crate::abac::set_abac_policy),
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
        .route(
            "/api/v1/repos/:id/commits/:sha/signature",
            get(jj::get_commit_signature),
        )
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
        .route("/api/v1/agents/:id/usage", post(spawn::record_agent_usage))
        .route("/api/v1/agents/:id/fail", post(spawn::fail_agent))
        .route("/api/v1/agents/:id/stop", post(spawn::stop_agent))
        .route(
            "/api/v1/agents/:id/card",
            get(get_agent_card).put(update_agent_card),
        )
        .route("/api/v1/agents/:id/messages", get(messages::poll_messages))
        .route(
            "/api/v1/agents/:id/messages/:message_id/ack",
            put(messages::ack_message),
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
        // Agent stack fingerprinting (M14.1)
        .route(
            "/api/v1/agents/:id/stack",
            post(stack_attest::register_stack).get(stack_attest::get_stack),
        )
        // Workload attestation (G10)
        .route("/api/v1/agents/:id/workload", get(workload::get_workload))
        // Container audit trail (M19.3)
        .route(
            "/api/v1/agents/:id/container",
            get(container::get_agent_container),
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
            "/api/v1/merge-requests/:id/attestation",
            get(merge_requests::get_attestation),
        )
        .route(
            "/api/v1/merge-requests/:id/gates",
            get(gates::list_mr_gate_results),
        )
        // MR SDLC timeline (S2.5 — HSI §3 System Trace View)
        .route(
            "/api/v1/merge-requests/:id/timeline",
            get(mr_timeline::get_mr_timeline),
        )
        // MR dependency graph (TASK-100)
        .route(
            "/api/v1/merge-requests/:id/dependencies",
            put(merge_deps::set_dependencies).get(merge_deps::get_dependencies),
        )
        .route(
            "/api/v1/merge-requests/:id/dependencies/:dependency_id",
            delete(merge_deps::remove_dependency),
        )
        .route(
            "/api/v1/merge-requests/:id/atomic-group",
            put(merge_deps::set_atomic_group),
        )
        // Gate-time trace capture (HSI §3a)
        .route(
            "/api/v1/merge-requests/:id/trace",
            get(traces::get_trace_for_mr),
        )
        .route(
            "/api/v1/trace-spans/:span_id/payload",
            get(traces::get_span_payload),
        )
        // Release automation (Admin only)
        .route("/api/v1/release/prepare", post(release::release_prepare))
        // Spec approval ledger (agent-gates spec)
        // NOTE: POST /api/v1/specs/approve and POST /api/v1/specs/revoke removed in M34 Slice 5
        //       (superseded by POST /api/v1/specs/:path/approve and POST /api/v1/specs/:path/revoke)
        .route("/api/v1/specs/approvals", get(gates::list_spec_approvals))
        // Spec registry (M21.1) — manifest-driven ledger
        .route("/api/v1/specs", get(specs::list_specs))
        .route("/api/v1/specs/pending", get(specs::list_pending_specs))
        .route("/api/v1/specs/drifted", get(specs::list_drifted_specs))
        .route("/api/v1/specs/index", get(specs::spec_index))
        .route("/api/v1/specs/graph", get(specs::get_spec_graph))
        // TASK-019: Spec link query endpoints (spec-links.md §Querying the Graph)
        // These fixed-path routes must be registered before `:path` routes.
        .route("/api/v1/specs/stale-links", get(specs::get_stale_links))
        .route("/api/v1/specs/conflicts", get(specs::get_conflicts))
        .route("/api/v1/specs/:path", get(specs::get_spec))
        .route("/api/v1/specs/:path/approve", post(specs::approve_spec))
        .route(
            "/api/v1/specs/:path/revoke",
            post(specs::revoke_spec_approval),
        )
        .route("/api/v1/specs/:path/reject", post(specs::reject_spec))
        .route(
            "/api/v1/specs/:path/history",
            get(specs::spec_approval_history),
        )
        .route("/api/v1/specs/:path/links", get(specs::get_spec_links))
        .route(
            "/api/v1/specs/:path/dependents",
            get(specs::get_spec_dependents),
        )
        .route(
            "/api/v1/specs/:path/dependencies",
            get(specs::get_spec_dependencies),
        )
        .route(
            "/api/v1/specs/:path/progress",
            get(specs::get_spec_progress),
        )
        // Constraint validation dry-run (authorization-provenance.md §7.6)
        .route(
            "/api/v1/constraints/validate",
            post(specs::validate_constraints),
        )
        // Constraint dry-run evaluation (authorization-provenance.md §7.6)
        .route(
            "/api/v1/constraints/dry-run",
            post(specs::dry_run_constraints),
        )
        // Strategy-implied constraints preview (authorization-provenance.md §7.6)
        .route(
            "/api/v1/constraints/strategy",
            get(specs::get_strategy_constraints),
        )
        // Spec editing backend (S3.3 — HSI §11 CLI/MCP parity)
        .route(
            "/api/v1/repos/:id/specs/assist",
            post(specs_assist::assist_spec),
        )
        .route(
            "/api/v1/repos/:id/specs/save",
            post(specs_assist::save_spec),
        )
        .route(
            "/api/v1/repos/:id/prompts/save",
            post(specs_assist::save_prompt),
        )
        // Merge Queue
        .route("/api/v1/merge-queue/enqueue", post(merge_queue::enqueue))
        .route(
            "/api/v1/merge-queue/graph",
            get(merge_deps::get_queue_graph),
        )
        .route("/api/v1/merge-queue", get(merge_queue::list_queue))
        .route("/api/v1/merge-queue/:id", delete(merge_queue::cancel_entry))
        // Auth / API keys / token introspection (M18)
        .route("/api/v1/auth/api-keys", post(auth::create_api_key))
        .route("/api/v1/auth/token-info", get(auth::token_info))
        // Key binding (TASK-006, authorization-provenance.md §2.3)
        .route(
            "/api/v1/auth/key-binding",
            post(key_binding::create_key_binding),
        )
        // Analytics
        .route(
            "/api/v1/analytics/events",
            post(analytics::record_event).get(analytics::query_events),
        )
        .route("/api/v1/analytics/count", get(analytics::count_events))
        .route("/api/v1/analytics/daily", get(analytics::daily_events))
        // Analytics Decision API (M23)
        .route("/api/v1/analytics/usage", get(analytics::usage))
        .route("/api/v1/analytics/compare", get(analytics::compare))
        .route("/api/v1/analytics/top", get(analytics::top_events))
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
        // BCP (Business Continuity)
        .route("/api/v1/admin/bcp/targets", get(admin::admin_bcp_targets))
        .route("/api/v1/admin/bcp/drill", post(admin::admin_bcp_drill))
        // Seed data
        .route("/api/v1/admin/seed", post(admin::admin_seed))
        // Search reindex (moved from /api/v1/search/reindex in M34 Slice 5)
        .route(
            "/api/v1/admin/search/reindex",
            post(search::reindex_handler),
        )
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
        // SSH Tunnels (G12) — reverse tunnels so air-gapped agents phone home
        .route(
            "/api/v1/admin/compute-targets/:id/tunnel",
            post(open_tunnel).get(list_tunnels),
        )
        .route(
            "/api/v1/admin/compute-targets/:id/tunnel/:tunnel_id",
            delete(close_tunnel),
        )
        // Compute targets — tenant-scoped (agent-runtime spec §3)
        .route(
            "/api/v1/compute-targets",
            post(compute_targets::create_compute_target).get(compute_targets::list_compute_targets),
        )
        .route(
            "/api/v1/compute-targets/:id",
            get(compute_targets::get_compute_target)
                .put(compute_targets::update_compute_target)
                .delete(compute_targets::delete_compute_target),
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
        .route(
            "/api/v1/network/peers/:id",
            put(network::update_peer_endpoint).delete(network::delete_peer),
        )
        .route("/api/v1/network/derp-map", get(network::derp_map))
        // Cross-repo dependency graph (M22.4)
        .route(
            "/api/v1/repos/:id/dependencies",
            get(dependencies::list_dependencies).post(dependencies::add_dependency),
        )
        .route(
            "/api/v1/repos/:id/dependencies/:dependency_id",
            delete(dependencies::delete_dependency),
        )
        .route(
            "/api/v1/repos/:id/dependents",
            get(dependencies::list_dependents),
        )
        .route(
            "/api/v1/repos/:id/blast-radius",
            get(dependencies::blast_radius),
        )
        .route("/api/v1/dependencies/graph", get(dependencies::get_graph))
        // Federation (G11)
        .route(
            "/api/v1/federation/trusted-issuers",
            get(federation::list_trusted_issuers),
        )
        // Budget governance (M22.2)
        .route(
            "/api/v1/workspaces/:id/budget",
            get(budget::get_workspace_budget).put(budget::set_workspace_budget),
        )
        .route("/api/v1/budget/summary", get(budget::budget_summary))
        // Search (M22.7)
        .route("/api/v1/search", get(search::search_handler))
        // Tenants (M34)
        .route(
            "/api/v1/tenants",
            post(tenants::create_tenant).get(tenants::list_tenants),
        )
        .route(
            "/api/v1/tenants/:id",
            get(tenants::get_tenant)
                .put(tenants::update_tenant)
                .delete(tenants::delete_tenant),
        )
        // Trust anchors (TASK-006, authorization-provenance.md §1.1)
        .route(
            "/api/v1/tenants/:id/trust-anchors",
            get(trust_anchors::list_trust_anchors).post(trust_anchors::create_trust_anchor),
        )
        .route(
            "/api/v1/tenants/:id/trust-anchors/:aid",
            get(trust_anchors::get_trust_anchor)
                .put(trust_anchors::update_trust_anchor)
                .delete(trust_anchors::delete_trust_anchor),
        )
        // Workspaces (M22.1)
        .route(
            "/api/v1/workspaces",
            post(workspaces::create_workspace).get(workspaces::list_workspaces),
        )
        .route(
            "/api/v1/workspaces/:id",
            get(workspaces::get_workspace)
                .put(workspaces::update_workspace)
                .delete(workspaces::delete_workspace),
        )
        .route(
            "/api/v1/workspaces/:id/repos",
            post(workspaces::add_repo_to_workspace).get(workspaces::list_workspace_repos),
        )
        // Workspace-scoped entity lists (M34 Slice 6 — primary access patterns per api-conventions.md §1.1)
        .route(
            "/api/v1/workspaces/:workspace_id/tasks",
            get(tasks::list_workspace_tasks),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents",
            get(agents::list_workspace_agents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/merge-requests",
            get(merge_requests::list_workspace_mrs),
        )
        // Meta-spec sets (M32)
        .route(
            "/api/v1/workspaces/:id/meta-spec-set",
            get(meta_specs::get_meta_spec_set).put(meta_specs::put_meta_spec_set),
        )
        // Meta-spec preview loop (S4.6 — §5 of meta-spec-reconciliation.md)
        // NOTE: the status route must be registered before the POST route to avoid
        // axum matching the preview_id segment as a method collision.
        .route(
            "/api/v1/workspaces/:id/meta-specs/preview",
            post(meta_specs::post_meta_spec_preview),
        )
        .route(
            "/api/v1/workspaces/:id/meta-specs/preview/:preview_id",
            get(meta_specs::get_meta_spec_preview_status),
        )
        // LLM function config (LLM integration §4)
        .route(
            "/api/v1/workspaces/:id/llm/config",
            get(llm_config::list_workspace_llm_configs),
        )
        .route(
            "/api/v1/workspaces/:id/llm/config/:function",
            get(llm_config::get_effective_llm_config)
                .put(llm_config::put_workspace_llm_config)
                .delete(llm_config::delete_workspace_llm_config),
        )
        .route(
            "/api/v1/admin/llm/config",
            get(llm_config::list_tenant_llm_defaults),
        )
        .route(
            "/api/v1/admin/llm/config/:function",
            put(llm_config::put_tenant_llm_default),
        )
        // Message bus (Phase 3)
        .route(
            "/api/v1/workspaces/:workspace_id/messages",
            post(messages::send_message).get(messages::list_workspace_messages),
        )
        // Presence (HSI §7)
        .route(
            "/api/v1/workspaces/:workspace_id/presence",
            get(workspaces::get_workspace_presence),
        )
        // LLM prompt templates (Slice 2)
        .route(
            "/api/v1/workspaces/:id/llm/prompts",
            get(llm_prompts::list_workspace_prompts),
        )
        .route(
            "/api/v1/workspaces/:id/llm/prompts/:function",
            get(llm_prompts::get_effective_prompt)
                .put(llm_prompts::upsert_workspace_prompt)
                .delete(llm_prompts::delete_workspace_prompt),
        )
        .route(
            "/api/v1/admin/llm/prompts",
            get(llm_prompts::list_tenant_defaults),
        )
        .route(
            "/api/v1/admin/llm/prompts/:function",
            put(llm_prompts::upsert_tenant_default),
        )
        // Meta-spec blast radius (M32)
        .route(
            "/api/v1/meta-specs/:path/blast-radius",
            get(meta_specs::get_meta_spec_blast_radius),
        )
        // Meta-spec registry CRUD (agent-runtime spec §2)
        // NOTE: /:id/versions must be registered before /:id to prevent axum from
        // matching "versions" as an id segment.
        .route(
            "/api/v1/meta-specs-registry",
            get(meta_specs::list_meta_specs_registry).post(meta_specs::create_meta_spec_registry),
        )
        .route(
            "/api/v1/meta-specs-registry/:id",
            get(meta_specs::get_meta_spec_registry)
                .put(meta_specs::update_meta_spec_registry)
                .delete(meta_specs::delete_meta_spec_registry),
        )
        .route(
            "/api/v1/meta-specs-registry/:id/versions",
            get(meta_specs::list_meta_spec_versions),
        )
        .route(
            "/api/v1/meta-specs-registry/:id/versions/:version",
            get(meta_specs::get_meta_spec_version),
        )
        // Personas (M22.1, VISION-3)
        .route(
            "/api/v1/personas",
            post(personas::create_persona).get(personas::list_personas),
        )
        .route("/api/v1/personas/resolve", get(personas::resolve_persona))
        .route(
            "/api/v1/personas/:id",
            get(personas::get_persona)
                .put(personas::update_persona)
                .delete(personas::delete_persona),
        )
        .route(
            "/api/v1/personas/:id/approve",
            post(personas::approve_persona),
        )
        // ABAC policy engine (M22.6)
        .route(
            "/api/v1/policies",
            get(policies::list_policies).post(policies::create_policy),
        )
        .route("/api/v1/policies/evaluate", post(policies::evaluate_policy))
        .route("/api/v1/policies/decisions", get(policies::list_decisions))
        .route(
            "/api/v1/policies/effective",
            get(policies::effective_permissions),
        )
        .route(
            "/api/v1/policies/:id",
            get(policies::get_policy)
                .put(policies::update_policy)
                .delete(policies::delete_policy),
        )
        // User profile (M22.8 + HSI §12)
        .route("/api/v1/users/me", get(get_me).put(update_me))
        .route("/api/v1/users/me/agents", get(get_my_agents))
        .route("/api/v1/users/me/tasks", get(get_my_tasks))
        .route("/api/v1/users/me/mrs", get(get_my_mrs))
        // API Tokens (HSI §12) — per-handler auth, ABAC-exempt
        .route(
            "/api/v1/users/me/tokens",
            get(list_tokens).post(create_token),
        )
        .route("/api/v1/users/me/tokens/:id", delete(delete_token))
        // Notification Preferences (HSI §12) — per-handler auth, ABAC-exempt
        .route(
            "/api/v1/users/me/notification-preferences",
            get(get_notification_preferences).put(update_notification_preferences),
        )
        // Judgment Ledger (HSI §12) — per-handler auth, ABAC-exempt
        .route("/api/v1/users/me/judgments", get(get_judgments))
        // Notifications (HSI §2) — per-handler auth, ABAC-exempt
        .route("/api/v1/users/me/notifications", get(get_my_notifications))
        .route(
            "/api/v1/users/me/notifications/count",
            get(get_notification_count),
        )
        .route(
            "/api/v1/notifications/:id/dismiss",
            post(dismiss_notification),
        )
        .route(
            "/api/v1/notifications/:id/resolve",
            post(resolve_notification),
        )
        // Workspace members (M22.8)
        .route(
            "/api/v1/workspaces/:id/members",
            post(invite_member).get(list_members),
        )
        .route(
            "/api/v1/workspaces/:id/members/:user_id",
            put(update_member_role).delete(remove_member),
        )
        // Teams (M22.8)
        .route(
            "/api/v1/workspaces/:id/teams",
            post(create_team).get(list_teams),
        )
        .route(
            "/api/v1/workspaces/:id/teams/:team_id",
            put(update_team).delete(delete_team),
        )
        // ── SCIM 2.0 Provisioning ─────────────────────────────────────────
        .route(
            "/scim/v2/Users",
            get(scim::scim_list_users).post(scim::scim_create_user),
        )
        .route(
            "/scim/v2/Users/:id",
            get(scim::scim_get_user)
                .put(scim::scim_update_user)
                .delete(scim::scim_delete_user),
        )
        .route(
            "/scim/v2/ServiceProviderConfig",
            get(scim::scim_service_provider_config),
        )
        .route("/scim/v2/Schemas", get(scim::scim_schemas))
        .route("/scim/v2/ResourceTypes", get(scim::scim_resource_types))
        // ── Knowledge graph (realized-model.md §7) ────────────────────────
        .route("/api/v1/repos/:id/graph", get(graph::get_repo_graph))
        .route("/api/v1/repos/:id/graph/types", get(graph::get_graph_types))
        .route(
            "/api/v1/repos/:id/graph/modules",
            get(graph::get_graph_modules),
        )
        .route(
            "/api/v1/repos/:id/graph/node/:node_id",
            get(graph::get_graph_node),
        )
        .route(
            "/api/v1/repos/:id/graph/spec/:spec_path",
            get(graph::get_graph_by_spec),
        )
        .route(
            "/api/v1/repos/:id/graph/concept/:concept_name",
            get(graph::get_graph_concept),
        )
        .route(
            "/api/v1/repos/:id/graph/timeline",
            get(graph::get_graph_timeline),
        )
        .route("/api/v1/repos/:id/graph/risks", get(graph::get_graph_risks))
        .route("/api/v1/repos/:id/graph/diff", get(graph::get_graph_diff))
        .route(
            "/api/v1/repos/:id/graph/link",
            post(graph::link_node_to_spec),
        )
        .route(
            "/api/v1/repos/:id/graph/predict",
            get(graph::predict_graph).post(graph::predict_graph),
        )
        .route(
            "/api/v1/repos/:id/graph/query-dryrun",
            post(graph::view_query_dryrun),
        )
        // ── Spec assertions (system-explorer S9) ─────────────────────────
        .route(
            "/api/v1/repos/:id/spec-assertions/check",
            post(spec_assertions::check_spec_assertions),
        )
        .route(
            "/api/v1/workspaces/:id/graph",
            get(graph::get_workspace_graph),
        )
        .route(
            "/api/v1/workspaces/:id/graph/concept/:concept_name",
            get(graph::get_workspace_graph_concept),
        )
        .route(
            "/api/v1/workspaces/:id/briefing",
            get(graph::get_workspace_briefing),
        )
        .route(
            "/api/v1/workspaces/:id/briefing/ask",
            post(graph::briefing_ask),
        )
        // Saved views (per-repo, explorer-implementation.md)
        .route(
            "/api/v1/repos/:id/views",
            get(saved_views::list_views).post(saved_views::create_view),
        )
        .route(
            "/api/v1/repos/:id/views/:view_id",
            get(saved_views::get_view)
                .put(saved_views::update_view)
                .delete(saved_views::delete_view),
        )
        // Explorer views CRUD + LLM generation (S3.1)
        // NOTE: /generate must be registered BEFORE /:view_id to avoid "generate"
        //       being matched as a view_id parameter.
        .route(
            "/api/v1/workspaces/:id/explorer-views",
            get(explorer_views::list_explorer_views).post(explorer_views::create_explorer_view),
        )
        .route(
            "/api/v1/workspaces/:id/explorer-views/generate",
            post(explorer_views::generate_explorer_view),
        )
        .route(
            "/api/v1/workspaces/:id/explorer-views/:view_id",
            get(explorer_views::get_explorer_view)
                .put(explorer_views::update_explorer_view)
                .delete(explorer_views::delete_explorer_view),
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
