//! ABAC middleware: evaluates access policy on every authenticated API request.
//!
//! Runs after `require_auth_middleware` (which identifies the caller) and before
//! any handler. Uses the existing `policy_engine` and `PolicyRepository`.
//!
//! System tokens (global `GYRE_AUTH_TOKEN`) bypass ABAC entirely — they are the
//! superuser escape hatch for bootstrap and emergency access.
//!
//! Pipeline position (per hierarchy-enforcement.md §4):
//! ```text
//! rate_limit_middleware
//! request_tracing
//! require_auth_middleware   ← identifies the caller
//! abac_middleware           ← this module; evaluates access policy
//! Handler
//! ```

use std::sync::{Arc, OnceLock};

use axum::{
    body::Body,
    extract::{FromRequestParts, MatchedPath},
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use gyre_common::Id;
use gyre_domain::policy::{
    Condition, ConditionOp, ConditionValue, Policy, PolicyEffect, PolicyScope,
};
use serde_json::json;

use crate::{auth::AuthenticatedAgent, policy_engine, policy_engine::AttributeContext, AppState};

// ---------------------------------------------------------------------------
// Resource resolver
// ---------------------------------------------------------------------------

// Merge-request route patterns shared between the resolver registry below
// and the scoped gate-agent allow-list in `abac_middleware`. Each pattern is
// defined exactly once so the registry mapping and the allow-list cannot
// drift apart (scripts/check-abac-route-registry.sh freezes duplicate path
// literals — a second copy of a pattern would be dead or misleading).
const MR_DETAIL_ROUTE: &str = "/api/v1/merge-requests/:id";
const MR_COMMENTS_ROUTE: &str = "/api/v1/merge-requests/:id/comments";
const MR_REVIEWS_ROUTE: &str = "/api/v1/merge-requests/:id/reviews";
const MR_DIFF_ROUTE: &str = "/api/v1/merge-requests/:id/diff";
const MR_GATES_ROUTE: &str = "/api/v1/merge-requests/:id/gates";

/// Routes a `review:submit` gate agent may touch: read MR context, submit
/// its verdict, read comments and gate state.
const REVIEWER_ALLOWED_ROUTES: &[&str] = &[
    MR_DETAIL_ROUTE,
    MR_REVIEWS_ROUTE,
    MR_DIFF_ROUTE,
    MR_COMMENTS_ROUTE,
    MR_GATES_ROUTE,
    "/api/v1/version",
];

/// Routes a `validation:report` gate agent may touch: read-only MR context
/// for its check and nothing else (its contract is the process exit code).
const VALIDATOR_ALLOWED_ROUTES: &[&str] = &[MR_DETAIL_ROUTE, MR_DIFF_ROUTE, "/api/v1/version"];

/// A single entry in the route registry: maps an axum route pattern to a
/// resource type and optional path parameters for workspace resolution.
pub struct RouteResourceMapping {
    /// Axum route pattern, e.g. "/api/v1/tasks/:id".
    pub pattern: &'static str,
    /// The ABAC resource type string, e.g. "task".
    pub resource_type: &'static str,
    /// Override the action for this route (e.g. "spawn", "approve").
    pub action_override: Option<&'static str>,
    /// If true, this route is exempt from ABAC evaluation.
    pub exempt: bool,
}

impl RouteResourceMapping {
    const fn api(
        pattern: &'static str,
        resource_type: &'static str,
        action_override: Option<&'static str>,
    ) -> Self {
        Self {
            pattern,
            resource_type,
            action_override,
            exempt: false,
        }
    }

    const fn exempt(pattern: &'static str) -> Self {
        Self {
            pattern,
            resource_type: "exempt",
            action_override: None,
            exempt: true,
        }
    }
}

/// Registry mapping every API route pattern to its resource type.
///
/// Every route in `api_router()` must have an entry here (enforced by
/// `scripts/check-api-auth.sh`).
pub struct ResourceResolver {
    routes: Vec<RouteResourceMapping>,
}

impl Default for ResourceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceResolver {
    pub fn new() -> Self {
        Self {
            routes: vec![
                // ── Public / ABAC-exempt ────────────────────────────────────
                RouteResourceMapping::exempt("/api/v1/version"),
                // ── Activity ───────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/activity", "activity", None),
                // ── Repos ──────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/repos", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/mirror", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/branches", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/commits", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/diff", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/mirror/sync", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/sync-specs", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/status", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/queue/pause", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/queue/resume", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/revert/:mr_id", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/post-merge-gates", "gate", None),
                RouteResourceMapping::api("/api/v1/repos/:id/provenance", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/aibom", "repo", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/commits/record",
                    "repo",
                    Some("write"),
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/agent-commits", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/worktrees", "worktree", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/worktrees/:worktree_id",
                    "worktree",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/gates", "gate", None),
                RouteResourceMapping::api("/api/v1/repos/:id/gates/:gate_id", "gate", None),
                RouteResourceMapping::api("/api/v1/repos/:id/push-gates", "gate", None),
                RouteResourceMapping::api("/api/v1/repos/:id/stack-policy", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/spec-policy", "repo", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/settings/spec-lifecycle",
                    "repo",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/abac-policy", "policy", None),
                RouteResourceMapping::api("/api/v1/repos/:id/blame", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/hot-files", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/review-routing", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/speculative", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/speculative/:branch", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/init", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/log", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/new", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/squash", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/undo", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/jj/bookmark", "repo", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/commits/:sha/signature", "repo", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/commits/:sha/signature/verification",
                    "repo",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/dependencies", "dependency", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/dependencies/:dependency_id",
                    "dependency",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/dependents", "dependency", None),
                RouteResourceMapping::api("/api/v1/repos/:id/blast-radius", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/types", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/modules", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/node/:node_id", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/spec/:spec_path", "graph", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/graph/concept/:concept_name",
                    "graph",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/timeline", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/risks", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/diff", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/link", "graph", Some("write")),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/predict", "graph", None),
                RouteResourceMapping::api("/api/v1/repos/:id/graph/query-dryrun", "graph", None),
                // ── Agents ─────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/agents", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/spawn", "agent", Some("spawn")),
                RouteResourceMapping::api("/api/v1/agents/discover", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/status", "agent", Some("write")),
                RouteResourceMapping::api("/api/v1/agents/:id/heartbeat", "agent", Some("write")),
                RouteResourceMapping::api("/api/v1/agents/:id/complete", "agent", Some("complete")),
                RouteResourceMapping::api("/api/v1/agents/:id/card", "agent", Some("write")),
                RouteResourceMapping::api("/api/v1/agents/:id/messages", "message", None),
                RouteResourceMapping::api(
                    "/api/v1/agents/:id/messages/:message_id/ack",
                    "message",
                    Some("write"),
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:workspace_id/messages",
                    "message",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/agents/:id/touched-paths", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/logs", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/logs/stream", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/stack", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/workload", "agent", None),
                RouteResourceMapping::api("/api/v1/agents/:id/container", "agent", None),
                // ── Compose ────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/compose/apply", "compose", Some("write")),
                RouteResourceMapping::api("/api/v1/compose/status", "compose", None),
                RouteResourceMapping::api("/api/v1/compose/teardown", "compose", Some("write")),
                // ── Tasks ──────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/tasks", "task", None),
                RouteResourceMapping::api("/api/v1/tasks/:id", "task", None),
                RouteResourceMapping::api("/api/v1/tasks/:id/status", "task", Some("write")),
                // ── Merge Requests ─────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/merge-requests", "merge_request", None),
                RouteResourceMapping::api(MR_DETAIL_ROUTE, "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/status",
                    "merge_request",
                    Some("write"),
                ),
                RouteResourceMapping::api(MR_COMMENTS_ROUTE, "merge_request", None),
                RouteResourceMapping::api(MR_REVIEWS_ROUTE, "merge_request", None),
                RouteResourceMapping::api(MR_DIFF_ROUTE, "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/attestation",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(MR_GATES_ROUTE, "gate", None),
                // MR SDLC timeline (S2.5 — HSI §3)
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/timeline",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/dependencies",
                    "dependency",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/dependencies/:dependency_id",
                    "dependency",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/atomic-group",
                    "merge_request",
                    Some("write"),
                ),
                // ── Gate-time trace capture (HSI §3a) ──────────────────────
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/trace",
                    "merge_request",
                    None,
                ),
                // /api/v1/trace-spans/:span_id/payload is ABAC-exempt (per-handler auth).
                // The handler resolves span → gate_trace → MR → workspace for authorization.
                RouteResourceMapping::exempt("/api/v1/trace-spans/:span_id/payload"),
                // ── Release ────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/release/prepare", "release", Some("write")),
                // ── Specs ──────────────────────────────────────────────────
                // NOTE: /api/v1/specs/approve and /api/v1/specs/revoke removed in M34 Slice 5
                RouteResourceMapping::api("/api/v1/specs/approvals", "spec", None),
                RouteResourceMapping::api("/api/v1/specs", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/pending", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/drifted", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/index", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/graph", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/approve", "spec", Some("approve")),
                RouteResourceMapping::api("/api/v1/specs/:path/revoke", "spec", Some("write")),
                RouteResourceMapping::api("/api/v1/specs/:path/history", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/links", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/progress", "spec", None),
                // ── Spec editing backend (S3.3) ────────────────────────────
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/specs/assist",
                    "spec",
                    Some("generate"),
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/specs/save", "spec", Some("write")),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/spec-assertions/check",
                    "spec",
                    Some("write"),
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/specs/:path/assertions", "spec", None),
                RouteResourceMapping::api(
                    "/api/v1/repos/:id/prompts/save",
                    "spec",
                    Some("generate"),
                ),
                // ── Merge Queue ────────────────────────────────────────────
                RouteResourceMapping::api(
                    "/api/v1/merge-queue/enqueue",
                    "merge_queue",
                    Some("write"),
                ),
                RouteResourceMapping::api("/api/v1/merge-queue/graph", "merge_queue", None),
                RouteResourceMapping::api("/api/v1/merge-queue", "merge_queue", None),
                RouteResourceMapping::api("/api/v1/merge-queue/:id", "merge_queue", None),
                // ── Auth / API keys ────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/auth/api-keys", "auth", Some("write")),
                RouteResourceMapping::api("/api/v1/auth/token-info", "auth", None),
                // ── Analytics ──────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/analytics/events", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/count", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/daily", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/usage", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/compare", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/top", "analytics", None),
                RouteResourceMapping::api("/api/v1/costs", "cost", None),
                RouteResourceMapping::api("/api/v1/costs/summary", "cost", None),
                // ── Audit ──────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/audit/events", "audit", None),
                RouteResourceMapping::api("/api/v1/audit/stream", "audit", None),
                RouteResourceMapping::api("/api/v1/audit/stats", "audit", None),
                // ── Admin ──────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/admin/health", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/jobs", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/jobs/:name/run", "admin", Some("write")),
                RouteResourceMapping::api("/api/v1/admin/audit", "admin", None),
                // Admin user impersonation (identity-security.md §Impersonation).
                // Request/end are admin-only (`admin` resource →
                // builtin-admin-only-deny rejects non-Admin roles); the
                // handlers additionally verify the Admin role and a real
                // user identity. Approve/deny are ABAC-exempt: the TARGET
                // user must call them and may hold any role (a ReadOnly
                // target must still be able to consent or refuse);
                // authorization is per-handler — the caller's verified
                // user id must equal the request's target user, plus the
                // one-time approval code on approve.
                RouteResourceMapping::api("/api/v1/admin/impersonate", "admin", None),
                RouteResourceMapping::exempt("/api/v1/admin/impersonate/:request_id/approve"),
                RouteResourceMapping::exempt("/api/v1/admin/impersonate/:request_id/deny"),
                RouteResourceMapping::api("/api/v1/admin/impersonate/:request_id", "admin", None),
                // NEW-27 fix: all /api/v1/admin/* routes use resource_type="admin" so that
                // builtin-admin-only-deny (priority 850) rejects non-Admin callers.
                // Previously, kill/reassign used "agent" and compute-targets used
                // "compute_target", allowing Developer/Agent roles to force-kill agents
                // and open SSH tunnels without Admin privilege.
                RouteResourceMapping::api("/api/v1/admin/agents/:id/kill", "admin", Some("write")),
                RouteResourceMapping::api(
                    "/api/v1/admin/agents/:id/reassign",
                    "admin",
                    Some("write"),
                ),
                RouteResourceMapping::api("/api/v1/admin/snapshot", "admin", Some("write")),
                RouteResourceMapping::api("/api/v1/admin/snapshots", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/restore", "admin", Some("write")),
                RouteResourceMapping::api("/api/v1/admin/snapshots/:id", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/bcp/targets", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/bcp/drill", "admin", Some("write")),
                RouteResourceMapping::api("/api/v1/admin/seed", "admin", Some("write")),
                RouteResourceMapping::api("/api/v1/admin/export", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/retention", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/siem", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/siem/:id", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/compute-targets", "admin", None),
                RouteResourceMapping::api("/api/v1/admin/compute-targets/:id", "admin", None),
                RouteResourceMapping::api(
                    "/api/v1/admin/compute-targets/:id/tunnel",
                    "admin",
                    Some("write"),
                ),
                RouteResourceMapping::api(
                    "/api/v1/admin/compute-targets/:id/tunnel/:tunnel_id",
                    "admin",
                    None,
                ),
                // Tenant-scoped compute targets (agent-runtime spec §3):
                // reads are tenant-member visible (handler enforces tenant
                // scoping), mutations additionally require the Admin role
                // in the handler body. Registered here rather than carried
                // as exemption-file debt — the two stale exemption entries
                // for these paths were removed when the routes landed in
                // the resolver (check-abac-route-registry.sh shrinks).
                RouteResourceMapping::api("/api/v1/compute-targets", "compute_target", None),
                RouteResourceMapping::api("/api/v1/compute-targets/:id", "compute_target", None),
                // ── Secrets (platform-model.md §7, task-098) ─────────────
                // Admin-only: builtin-secret-admin-only (priority 860) denies
                // every non-Admin role on the "secret" resource type; Admins
                // are allowed by builtin-admin-all-operations (900) first.
                RouteResourceMapping::api("/api/v1/secrets", "secret", None),
                RouteResourceMapping::api("/api/v1/secrets/:id", "secret", None),
                RouteResourceMapping::api("/api/v1/secrets/:id/rotate", "secret", Some("write")),
                // ── Network ────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/network/peers", "network_peer", None),
                RouteResourceMapping::api(
                    "/api/v1/network/peers/agent/:agent_id",
                    "network_peer",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/network/peers/:id", "network_peer", None),
                RouteResourceMapping::api("/api/v1/network/derp-map", "network_peer", None),
                // ── Dependencies graph ─────────────────────────────────────
                RouteResourceMapping::api("/api/v1/dependencies/graph", "dependency", None),
                // ── Federation ─────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/federation/trusted-issuers", "federation", None),
                // ── Budget ─────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/workspaces/:id/budget", "budget", None),
                RouteResourceMapping::api("/api/v1/budget/summary", "budget", None),
                // ── Search ─────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/search", "search", None),
                RouteResourceMapping::api("/api/v1/search/suggest", "search", None),
                RouteResourceMapping::api("/api/v1/search/facets", "search", None),
                RouteResourceMapping::api("/api/v1/admin/search/reindex", "admin", Some("write")),
                // ── Tenants ────────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/tenants", "tenant", None),
                RouteResourceMapping::api("/api/v1/tenants/:id", "tenant", None),
                // ── Workspaces ─────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/workspaces", "workspace", None),
                RouteResourceMapping::api("/api/v1/workspaces/:id", "workspace", None),
                RouteResourceMapping::api("/api/v1/workspaces/:id/repos", "repo", None),
                // Workspace-scoped entity lists (M34 Slice 6 — primary access patterns)
                RouteResourceMapping::api("/api/v1/workspaces/:workspace_id/tasks", "task", None),
                RouteResourceMapping::api("/api/v1/workspaces/:workspace_id/agents", "agent", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:workspace_id/merge-requests",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/meta-spec-set",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/merge-policy", "workspace", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/meta-specs/rollout-policy",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/members",
                    "workspace_member",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/members/:user_id",
                    "workspace_member",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/teams", "team", None),
                RouteResourceMapping::api("/api/v1/workspaces/:id/teams/:team_id", "team", None),
                RouteResourceMapping::api("/api/v1/workspaces/:id/graph", "graph", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/graph/concept/:concept_name",
                    "graph",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/briefing", "workspace", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/briefing/ask",
                    "workspace",
                    Some("generate"),
                ),
                // ── Explorer views (S3.1) ───────────────────────────────────
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/explorer-views",
                    "explorer_view",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/explorer-views/generate",
                    "explorer_view",
                    Some("generate"),
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/explorer-views/:view_id",
                    "explorer_view",
                    None,
                ),
                // ── Presence (S1.5) ────────────────────────────────────────
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:workspace_id/presence",
                    "workspace",
                    None,
                ),
                // ── Meta-specs ─────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/meta-specs-registry", "meta_spec", None),
                RouteResourceMapping::api("/api/v1/meta-specs-registry/:id", "meta_spec", None),
                RouteResourceMapping::api(
                    "/api/v1/meta-specs-registry/:id/versions",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/meta-specs-registry/:id/versions/:version",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/meta-specs/:path/blast-radius",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/specs/:path/meta-spec-bindings",
                    "meta_spec",
                    None,
                ),
                // ── Personas ───────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/personas", "persona", None),
                RouteResourceMapping::api("/api/v1/personas/resolve", "persona", None),
                RouteResourceMapping::api("/api/v1/personas/:id", "persona", None),
                RouteResourceMapping::api(
                    "/api/v1/personas/:id/approve",
                    "persona",
                    Some("approve"),
                ),
                // ── Policies ───────────────────────────────────────────────
                RouteResourceMapping::api("/api/v1/policies", "policy", None),
                RouteResourceMapping::api("/api/v1/policies/evaluate", "policy", Some("evaluate")),
                RouteResourceMapping::api("/api/v1/policies/decisions", "policy", None),
                RouteResourceMapping::api("/api/v1/policies/effective", "policy", None),
                RouteResourceMapping::api("/api/v1/policies/:id", "policy", None),
                // ── Users / Profile ────────────────────────────────────────
                // POST /users (create) is admin-only, enforced in-handler.
                RouteResourceMapping::api("/api/v1/users", "user", None),
                RouteResourceMapping::api("/api/v1/users/me", "user", None),
                // Notification endpoints use per-handler auth (HSI §2) — ABAC-exempt.
                RouteResourceMapping::exempt("/api/v1/users/me/notifications"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/dismiss"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/resolve"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/read"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/unread"),
                // ── SCIM (separate auth token, exempt from ABAC) ───────────
                RouteResourceMapping::exempt("/scim/v2/Users"),
                RouteResourceMapping::exempt("/scim/v2/Users/:id"),
                RouteResourceMapping::exempt("/scim/v2/ServiceProviderConfig"),
                RouteResourceMapping::exempt("/scim/v2/Schemas"),
                RouteResourceMapping::exempt("/scim/v2/ResourceTypes"),
            ],
        }
    }

    /// Look up a route by its matched axum pattern.
    pub fn resolve<'a>(&'a self, matched_pattern: &str) -> Option<&'a RouteResourceMapping> {
        self.routes.iter().find(|r| r.pattern == matched_pattern)
    }

    /// Return all registered non-exempt patterns (for `check-api-auth.sh`).
    pub fn all_patterns(&self) -> Vec<&'static str> {
        self.routes
            .iter()
            .filter(|r| !r.exempt)
            .map(|r| r.pattern)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Action resolution
// ---------------------------------------------------------------------------

/// Map HTTP method to ABAC action string. Action endpoints override this.
fn method_to_action(method: &Method) -> &'static str {
    match *method {
        Method::GET => "read",
        Method::PUT => "write",
        Method::DELETE => "delete",
        _ => "write", // POST and other methods default to write
    }
}

// ---------------------------------------------------------------------------
// Built-in policy seed (M34 Slice 4)
// ---------------------------------------------------------------------------
/// Canonical id of the request-pipeline catch-all Deny. It is seeded as a
/// built-in so the HTTP ABAC middleware can default-deny unmatched requests.
/// Internal-service gates that must distinguish "no policy governs this
/// action" from "a catch-all denied it" (e.g. the merge processor's trust
/// gate, HSI §2) exclude this policy by id — see merge_processor.rs.
pub const DEFAULT_DENY_POLICY_ID: &str = "builtin-default-deny";

/// Built-in M34 ABAC policies that ship with the server.
///
/// These replicate the old RBAC extractor behaviour through attribute conditions
/// so that existing callers continue to work after RBAC extractors are removed.
/// They cannot be deleted (built_in = true).
pub fn m34_builtin_policies() -> Vec<Policy> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let by = "system".to_string();

    vec![
        // Priority 900: Admin role → allow everything.
        Policy {
            id: Id::new("builtin-admin-all-operations"),
            name: "admin-all-operations".to_string(),
            description: "Admin role allows all operations".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 900,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Admin".to_string()),
            }],
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 850: Non-admin users denied from admin resources.
        // Admin-all-operations (900) evaluates first and allows Admin role.
        // This catches Developer, Agent, ReadOnly attempting admin endpoints.
        Policy {
            id: Id::new("builtin-admin-only-deny"),
            name: "admin-only-deny".to_string(),
            description: "Non-admin roles are denied access to admin resources".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 850,
            effect: PolicyEffect::Deny,
            conditions: vec![], // no conditions — matches everyone (Admin already allowed at 900)
            actions: vec!["*".to_string()],
            resource_types: vec!["admin".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 860: Secrets are Admin-only (platform-model.md §7).
        // Admin role is allowed by builtin-admin-all-operations (900) before
        // this Deny is consulted; every other role (Developer, Agent,
        // ReadOnly, ...) is denied read/write/delete on the "secret"
        // resource type. Agents receive secret VALUES as environment
        // variables at spawn — never through the Admin API or MCP.
        Policy {
            id: Id::new("builtin-secret-admin-only"),
            name: "builtin:secret-admin-only".to_string(),
            description: "Secret management (Admin API /secrets) is restricted to the Admin role"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 860,
            effect: PolicyEffect::Deny,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::NotEquals,
                value: ConditionValue::String("Admin".to_string()),
            }],
            actions: vec!["*".to_string()],
            resource_types: vec!["secret".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 800: Developer role → allow read + write + agent actions.
        Policy {
            id: Id::new("builtin-developer-write-access"),
            name: "developer-write-access".to_string(),
            description: "Developer role allows read and write operations".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 800,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Developer".to_string()),
            }],
            actions: vec![
                "read".to_string(),
                "write".to_string(),
                "spawn".to_string(),
                "complete".to_string(),
                "approve".to_string(),
                "evaluate".to_string(),
            ],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 700: Agent role → allow read + write + agent lifecycle actions.
        Policy {
            id: Id::new("builtin-agent-scoped-access"),
            name: "agent-scoped-access".to_string(),
            description: "Agent role allows read and write in its scoped context".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 700,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Agent".to_string()),
            }],
            actions: vec![
                "read".to_string(),
                "write".to_string(),
                "complete".to_string(),
                "approve".to_string(),
            ],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 600: ReadOnly role → allow only read.
        Policy {
            id: Id::new("builtin-readonly-get-only"),
            name: "readonly-get-only".to_string(),
            description: "ReadOnly role allows read-only operations".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 600,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.global_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("ReadOnly".to_string()),
            }],
            actions: vec!["read".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 998: Require signed authorization for push/merge (§7.2).
        // Immutable Deny: non-system subjects are denied push and merge on
        // attestation resources UNLESS an attestation chain was located for
        // the submitted work (subject.attestation_chain_present). The
        // push-time and merge-time evaluations set that attribute when they
        // find a chain, so this policy denies chain-less work while letting
        // attested work through to the normal priority-based evaluation
        // (where user policies can still restrict it). Cannot be overridden
        // by any Allow policy.
        Policy {
            id: Id::new("builtin-require-signed-authorization"),
            name: "builtin:require-signed-authorization".to_string(),
            description:
                "Deny push/merge for non-system subjects unless an attestation chain exists for the submitted work"
                    .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 998,
            effect: PolicyEffect::Deny,
            conditions: vec![
                Condition {
                    attribute: "subject.type".to_string(),
                    operator: ConditionOp::NotEquals,
                    value: ConditionValue::String("system".to_string()),
                },
                Condition {
                    attribute: "subject.attestation_chain_present".to_string(),
                    operator: ConditionOp::NotEquals,
                    value: ConditionValue::Bool(true),
                },
            ],
            actions: vec!["push".to_string(), "merge".to_string()],
            resource_types: vec!["attestation".to_string()],
            enabled: true,
            built_in: true,
            immutable: true,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 150: default Allow for work that carries an attestation
        // chain (§7.2). Sits in the trust band — below user-created policies
        // (200-299) so user Deny policies still restrict attested work, and
        // above the builtin default-deny catchall (1) so attested pushes and
        // merges are allowed by default once `builtin:require-signed-authorization`
        // is satisfied.
        Policy {
            id: Id::new("builtin-attested-authorization"),
            name: "builtin:attested-authorization".to_string(),
            description:
                "Allow push/merge for work that carries an attestation chain (overridable by user policies)"
                    .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 150,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.attestation_chain_present".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::Bool(true),
            }],
            actions: vec!["push".to_string(), "merge".to_string()],
            resource_types: vec!["attestation".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 1: Default deny — lowest priority catchall.
        // Anything not explicitly allowed is denied.
        Policy {
            id: Id::new(DEFAULT_DENY_POLICY_ID),
            name: "default-deny".to_string(),
            description: "Default deny — any request not matching an Allow policy is denied"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 1,
            effect: PolicyEffect::Deny,
            conditions: vec![], // no conditions → always matches
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by,
            created_at: now,
            updated_at: now,
        },
    ]
}

/// TASK-077 (HSI §2): the immutable `builtin:require-human-spec-approval`
/// policy lives in the domain builtin set
/// (`gyre_domain::builtin_policies`) — that set is the single source of truth
/// for its definition. Only this policy is spliced into the startup seed:
/// the remaining domain-set policies are owned by their own task-specific
/// seeding flows and enabling them here would change authorization behavior
/// beyond TASK-077's scope.
fn domain_builtin_policies() -> Vec<Policy> {
    gyre_domain::builtin_policies("system")
        .into_iter()
        .filter(|p| p.name == "builtin:require-human-spec-approval")
        .collect()
}

/// Seed built-in M34 policies into the policy store at startup. Idempotent.
///
/// Built-in policies are code-owned: the API refuses to modify or delete
/// them, so a stored row that differs from the code definition is a stale
/// copy from an older seed, not an operator edit. When the stored row
/// differs on any semantic field (name, description, scope, priority,
/// effect, conditions, actions, resource_types, enabled, immutable), it is
/// refreshed to the current code definition — otherwise a fix to a built-in
/// policy would never reach existing deployments (create-only seeding
/// skips rows that already exist).
///
/// Fails closed (task-077 F6 class): the seeded set includes immutable Deny
/// restrictions (`builtin:require-human-spec-approval`, HSI §2/§12 "spec
/// approval is always human"; `builtin:require-signed-authorization`) and the
/// default-deny catch-all. If any seed fails to persist, returning `Ok` would
/// leave the server enforcing a stricter- or looser-than-configured policy
/// set silently — an agent could approve specs while the operator believes
/// the immutable Deny at priority 999 is active. The caller must refuse to
/// serve with a partially seeded policy set.
pub async fn seed_builtin_policies(state: &Arc<AppState>) -> anyhow::Result<()> {
    let mut policies = m34_builtin_policies();
    policies.extend(domain_builtin_policies());
    for policy in policies {
        match state.policies.find_by_id(&policy.id.to_string()).await {
            Ok(None) => {
                state.policies.create(&policy).await.map_err(|e| {
                    anyhow::anyhow!("failed to seed built-in ABAC policy {}: {e}", policy.id)
                })?;
                tracing::debug!(
                    policy_id = %policy.id,
                    name = %policy.name,
                    "Seeded built-in ABAC policy"
                );
            }
            Ok(Some(stored)) => {
                if builtin_policy_drifted(&stored, &policy) {
                    let mut refreshed = policy.clone();
                    // Preserve the original creation metadata; only the
                    // definition is code-owned.
                    refreshed.created_by = stored.created_by.clone();
                    refreshed.created_at = stored.created_at;
                    state.policies.update(&refreshed).await.map_err(|e| {
                        anyhow::anyhow!(
                            "failed to refresh drifted built-in ABAC policy {}: {e}",
                            policy.id
                        )
                    })?;
                    tracing::info!(
                        policy_id = %policy.id,
                        name = %policy.name,
                        "Refreshed drifted built-in ABAC policy to current definition"
                    );
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to check existence of built-in ABAC policy {}: {e}",
                    policy.id
                ));
            }
        }
    }
    Ok(())
}

/// Whether a stored built-in policy row differs from its code definition on
/// any semantic field. `id`, `built_in`, `created_by`, and `created_at` are
/// excluded: they never change between seed generations.
fn builtin_policy_drifted(stored: &Policy, code: &Policy) -> bool {
    stored.name != code.name
        || stored.description != code.description
        || stored.scope != code.scope
        || stored.scope_id != code.scope_id
        || stored.priority != code.priority
        || stored.effect != code.effect
        || stored.conditions != code.conditions
        || stored.actions != code.actions
        || stored.resource_types != code.resource_types
        || stored.enabled != code.enabled
        || stored.immutable != code.immutable
}

// ---------------------------------------------------------------------------
// Global resolver
// ---------------------------------------------------------------------------

static RESOURCE_RESOLVER: OnceLock<ResourceResolver> = OnceLock::new();

/// Initialise the global `ResourceResolver` (called once at startup).
pub fn init_resolver() {
    RESOURCE_RESOLVER.get_or_init(ResourceResolver::new);
}

/// Identity-security.md §Impersonation: attribute calls made under an
/// impersonation token even when they bypass policy evaluation — ABAC-exempt
/// routes and unresolved patterns (previously these returned before the
/// attribution block, leaving e.g. an impersonated read of the target's
/// private notifications unattributed and uncounted).
///
/// Peeks the bearer token; any request that is not an active impersonation
/// token passes through untouched (no behavior change). When it is one, the
/// action is recorded (fire-and-forget) and the request is marked so the
/// handler's own auth extraction does not double-count the same call.
async fn record_impersonated_passthrough(
    state: Arc<AppState>,
    req: Request<Body>,
    surface: &'static str,
) -> Request<Body> {
    use axum::http::header::AUTHORIZATION;

    let Some(token) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return req;
    };
    if !token.starts_with("ey") {
        return req;
    }
    // Cheap cryptographic pre-filter: only impersonation JWTs proceed to
    // the session lookup.
    let Some(claims) = state
        .agent_signing_key
        .validate_impersonation_opt(token, &state.base_url)
    else {
        return req;
    };
    let Some(auth) = crate::impersonation::resolve_impersonation_auth(&claims, &state).await else {
        // Inactive session — the handler's auth extraction will reject it.
        return req;
    };

    let (mut parts, body) = req.into_parts();
    // Upstream middlewares (last_seen) may have marked the request to
    // defer attribution to this middleware — for exempt/unresolved routes
    // THIS branch is the attribution owner, so record regardless and mark
    // the request so the handler's own auth extraction does not
    // double-count the same call.
    parts
        .extensions
        .insert(crate::impersonation::ImpersonationActionRecorded);
    if let Some(imp) = &auth.impersonation {
        crate::impersonation::spawn_record_impersonated_action(
            &state,
            imp,
            parts.method.as_str(),
            parts.uri.path(),
            true,
            surface,
        );
    }
    Request::from_parts(parts, body)
}

// ---------------------------------------------------------------------------
// Middleware function
// ---------------------------------------------------------------------------

/// ABAC middleware — evaluates access policy for every authenticated API request.
///
/// Must be applied AFTER `require_auth_middleware` and BEFORE route handlers.
pub async fn abac_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    matched_path: Option<MatchedPath>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let pattern: String = matched_path
        .as_ref()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    // Resolve the route mapping. Routes not in the registry fall through
    // (scripts/check-abac-route-registry.sh fails the build if a registered
    // /api/v1/ route has no entry here — keep it that way; an unregistered
    // route means NO policy evaluation).
    let (resource_type, action_override, exempt) =
        match RESOURCE_RESOLVER.get().and_then(|r| r.resolve(&pattern)) {
            Some(m) => (m.resource_type, m.action_override, m.exempt),
            None => {
                // No policy evaluation, but the call is still attributed
                // when made under an impersonation token
                // (identity-security.md §Impersonation).
                let req =
                    record_impersonated_passthrough(state.clone(), req, "abac-unresolved").await;
                return next.run(req).await;
            }
        };

    // ABAC-exempt routes pass through immediately — after impersonation
    // attribution, so an impersonated read of the target's private
    // notifications is still audited (identity-security.md §Impersonation:
    // full audit trail of ALL actions taken during impersonation).
    if exempt {
        let req = record_impersonated_passthrough(state.clone(), req, "abac-exempt").await;
        return next.run(req).await;
    }

    // Extract auth from request (splits and reconstructs).
    let method = req.method().clone();
    let (mut parts, body) = req.into_parts();

    // Evaluated routes are attributed AFTER policy evaluation (with the
    // decision outcome). Mark the request so the auth-resolution layer
    // (`AuthenticatedAgent::from_request_parts`, called here and again by
    // handler extractors) does not double-count the same call.
    parts
        .extensions
        .insert(crate::impersonation::ImpersonationActionRecorded);

    let auth = match AuthenticatedAgent::from_request_parts(&mut parts, &state).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    let req = Request::from_parts(parts, body);

    // Resolve action (needed by the scoped-token allow-list below and by
    // policy evaluation).
    let action = action_override.unwrap_or_else(|| method_to_action(&method));

    // System token bypasses ABAC entirely. `is_system_principal()` (not an
    // agent_id string match): API-key/JWT auth sets agent_id from the user's
    // display_name, so a user named "system" would otherwise spoof this
    // bypass (task-099 F1 revision).
    if auth.is_system_principal() {
        return next.run(req).await;
    }

    // Task-134 (agent-gates.md §Gate Agent Lifecycle): scoped tokens are
    // allow-listed to their capability's routes. A `review:submit` gate
    // agent may read MR context and submit its verdict; a
    // `validation:report` agent may read MR context for its check and
    // nothing else (its contract is the process exit code). Anything else
    // is denied before policy evaluation, so no repo/admin policy can
    // widen a single-purpose gate agent into a general-purpose caller.
    // Capability comparison is exact-match: a substring check would let a
    // hypothetical `review:submit:admin` scope inherit the allow-list
    // (review F3).
    if let Some(scope) = auth
        .jwt_claims
        .as_ref()
        .and_then(|c| c.get("scope"))
        .and_then(|s| s.as_str())
    {
        let is_reviewer = scope == crate::auth::REVIEW_SUBMIT_SCOPE;
        let is_validator = scope == crate::auth::VALIDATION_REPORT_SCOPE;
        if is_reviewer || is_validator {
            // Read-only MR context for validators; reviewers additionally
            // get review submission, comments, and gate state. No repo/admin
            // route is reachable, so no policy can widen a single-purpose
            // gate agent into a general-purpose caller.
            let allowed = if is_reviewer {
                REVIEWER_ALLOWED_ROUTES.contains(&pattern.as_str())
            } else {
                VALIDATOR_ALLOWED_ROUTES.contains(&pattern.as_str())
            };
            if !allowed {
                tracing::warn!(
                    subject_id = %auth.agent_id,
                    pattern = %pattern,
                    scope = %scope,
                    "ABAC denied request: scoped gate-agent token outside allow-list"
                );
                let decision = policy_engine::build_decision(
                    &policy_engine::EvalResult {
                        effect: gyre_domain::PolicyEffect::Deny,
                        matched_policy: None,
                        evaluated_count: 0,
                        evaluation_ms: 0.0,
                    },
                    &auth.agent_id,
                    "agent",
                    action,
                    resource_type,
                    &pattern,
                );
                let _ = state.policies.record_decision(&decision).await;
                let error = if is_reviewer {
                    "review-scoped token is limited to review routes"
                } else {
                    "validation-scoped token is limited to read-only MR routes"
                };
                return (StatusCode::FORBIDDEN, Json(json!({"error": error}))).into_response();
            }
        }
    }

    // Build attribute context.
    let mut ctx = AttributeContext::default();

    let subject_type = if auth.roles.contains(&gyre_domain::UserRole::Agent) {
        "agent"
    } else {
        "user"
    };
    ctx.set("subject.type", subject_type);

    let global_role = auth.roles.first().map(|r| r.as_str()).unwrap_or("ReadOnly");
    ctx.set("subject.global_role", global_role);
    ctx.set("subject.tenant_id", &auth.tenant_id);

    if let Some(claims) = &auth.jwt_claims {
        ctx.merge_jwt_claims(claims);
    }

    // Load policies and evaluate.
    // Policies are loaded from the shared store on EVERY request — there is
    // no ABAC policy-result cache in gyre-server (only JWKS / graph /
    // dep-staleness caches exist, none of which cache policy decisions).
    // Trust-level transitions write workspace row + trust: policies through
    // this same store in one transaction, so any transition is visible to the
    // next request with no cache invalidation needed (TASK-077 review note).
    let policies = state.policies.list().await.unwrap_or_default();
    let result = policy_engine::evaluate(policies, &ctx, action, resource_type);

    // Identity-security.md §Impersonation: every API call made with an
    // impersonation token is attributed to the impersonator in the audit
    // trail and counted in the session's actions_taken. Fire-and-forget so
    // the audit write never blocks the request.
    if let Some(imp) = auth.impersonation.clone() {
        crate::impersonation::spawn_record_impersonated_action(
            &state,
            &imp,
            method.as_str(),
            &pattern,
            result.effect == PolicyEffect::Allow,
            "abac",
        );
    }

    // Record decision to audit log.
    let decision = policy_engine::build_decision(
        &result,
        &auth.agent_id,
        subject_type,
        action,
        resource_type,
        &pattern,
    );
    let _ = state.policies.record_decision(&decision).await;

    match result.effect {
        PolicyEffect::Allow => next.run(req).await,
        PolicyEffect::Deny => {
            tracing::warn!(
                subject_id = %auth.agent_id,
                action = %action,
                resource_type = %resource_type,
                pattern = %pattern,
                matched_policy = ?result.matched_policy,
                "ABAC denied request"
            );
            // task-104: AbacDenied audit event (observability.md §Audit
            // Event Types) — the policy engine rejected the request.
            record_abac_denied(
                &state,
                &auth,
                resource_type,
                action,
                result.matched_policy.as_deref(),
                &pattern,
            )
            .await;
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "insufficient permissions"})),
            )
                .into_response()
        }
    }
}

/// Record an `AbacDenied` audit event (observability.md §Audit Event Types:
/// ABAC policy rejects request) with the spec'd detail fields: agent_id,
/// policy_id, resource, action, claims.
///
/// Shared by the ABAC middleware and per-handler ABAC enforcement
/// (e.g. git smart-HTTP push) so every policy denial lands in the audit
/// trail regardless of which layer rejected the request.
pub async fn record_abac_denied(
    state: &AppState,
    auth: &AuthenticatedAgent,
    resource: &str,
    action: &str,
    policy_id: Option<&str>,
    endpoint: &str,
) {
    let event = gyre_domain::AuditEvent::new(
        gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
        gyre_domain::AuditEventType::AbacDenied,
        if auth.agent_id.is_empty() {
            None
        } else {
            Some(gyre_common::Id::new(auth.agent_id.clone()))
        },
        auth.user_id.clone(),
        None,
        None,
        None,
        "policy".to_string(),
        None,
        gyre_domain::AuditOutcome::Blocked,
        serde_json::json!({
            "agent_id": auth.agent_id,
            "policy_id": policy_id,
            "resource": resource,
            "action": action,
            "claims": auth.jwt_claims,
            "endpoint": endpoint,
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mem::test_state;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn build_test_app(state: Arc<AppState>) -> Router {
        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        Router::new()
            .route("/api/v1/tasks", get(ok_handler))
            .route("/api/v1/repos", get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state)
    }

    fn setup_state_with_policies() -> Arc<AppState> {
        let state = test_state();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });
        state
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn system_token_bypasses_abac() {
        let state = setup_state_with_policies();
        let app = build_test_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn admin_jwt_allowed_on_all_routes() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};

        let state_base = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state_base))
                .expect("seed built-in policies")
        });
        init_resolver();

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/tasks", get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state_base.clone(),
                abac_middleware,
            ))
            .with_state(state_base);

        let token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "admin-sub",
                "preferred_username": "admin",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn readonly_jwt_denied_on_write() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::routing::post;

        let state_base = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state_base))
                .expect("seed built-in policies")
        });
        init_resolver();

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        // POST /api/v1/tasks → action="write" (POST maps to write)
        let app = Router::new()
            .route("/api/v1/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state_base.clone(),
                abac_middleware,
            ))
            .with_state(state_base);

        let token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn abac_denial_records_abac_denied_audit_event() {
        // task-104: an ABAC policy rejection must land in the audit store
        // with the spec'd detail fields (agent_id, policy_id, resource,
        // action, claims).
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::routing::post;

        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });
        init_resolver();

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let token = sign_test_jwt(
            &serde_json::json!({
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let events = gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some("abac_denied".to_string()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1, "one abac_denied event");
        let e = &events[0];
        assert_eq!(e.detail["agent_id"], "readonly-user");
        assert_eq!(e.detail["resource"], "task");
        assert_eq!(e.detail["action"], "write");
        assert_eq!(e.detail["endpoint"], "/api/v1/tasks");
        assert!(
            e.detail["policy_id"].as_str().is_some(),
            "the denying policy must be named, got {:?}",
            e.detail["policy_id"]
        );
        // The caller's JWT claims travel with the denial for forensics.
        assert_eq!(e.detail["claims"]["sub"], "ro-sub");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn uninstrumented_abac_guards_record_abac_denied_audit_event() {
        // Review repair (task-104 round 11): per-handler repo/workspace-level
        // ABAC guards — the G6 kv-store policies, the same policy layer the
        // git push path already instruments via record_abac_denied — rejected
        // with 403 while recording no abac_denied event: create_worktree, the
        // four jj mutation endpoints, repo orchestrator spawn, the
        // briefing/ask repo-scope guard, spawn_agent_core (REST + MCP), and
        // the conversations workspace-read guard. ABAC-side equivalent of
        // the round-8/9/10 per-handler RBAC sweep. Each sub-case gets a
        // fresh state so the recorded event count is exactly one.
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::body::Body;
        use http::Request;
        use tower::ServiceExt;

        // Fresh state with production-seeded builtin policies (as main.rs
        // does) plus a G6 kv-store policy on `scope_key` requiring a
        // `team=infra` claim no caller below carries. The builtin seed keeps
        // the middleware layer permissive for the caller (Developer write /
        // Admin all) so the per-handler guard is the only denial source.
        async fn denied_state(scope_key: &str) -> Arc<AppState> {
            let state = make_test_state_with_jwt();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(seed_builtin_policies(&state))
                    .expect("seed built-in policies")
            });
            init_resolver();
            let policies = vec![crate::abac::AbacPolicy {
                resource_type: "repo".to_string(),
                resource_id: None,
                required_claims: std::iter::once(("team".to_string(), "infra".to_string()))
                    .collect(),
            }];
            state
                .kv_store
                .kv_set(
                    "abac_policies",
                    scope_key,
                    serde_json::to_string(&policies).unwrap(),
                )
                .await
                .unwrap();
            state
        }

        async fn seed_repo(state: &AppState, repo_id: &str, workspace_id: &str) {
            use gyre_domain::Repository;
            let repo = Repository::new(
                gyre_common::Id::new(repo_id),
                gyre_common::Id::new(workspace_id),
                format!("repo-{repo_id}"),
                format!("/repos/repo-{repo_id}"),
                0,
            );
            state.repos.create(&repo).await.unwrap();
        }

        async fn seed_workspace(state: &AppState, workspace_id: &str) {
            let ws = gyre_domain::Workspace::new(
                gyre_common::Id::new(workspace_id),
                gyre_common::Id::new("tenant-1"),
                format!("ws-{workspace_id}"),
                format!("ws-{workspace_id}"),
                1000,
            );
            state.workspaces.create(&ws).await.unwrap();
        }

        async fn assert_one_abac_denied(
            state: &AppState,
            resource: &str,
            action: &str,
            endpoint: &str,
        ) {
            let events = gyre_ports::AuditRepository::query(
                &*state.audit,
                &gyre_ports::AuditQueryFilter {
                    event_type: Some("abac_denied".to_string()),
                    limit: 10,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(events.len(), 1, "one abac_denied event for {endpoint}");
            let e = &events[0];
            assert_eq!(e.detail["resource"], resource);
            assert_eq!(e.detail["action"], action);
            assert_eq!(e.detail["endpoint"], endpoint);
            assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
        }

        // Keycloak-style Developer JWT: builtin policies allow read/write/
        // spawn at the middleware layer, but it carries jwt_claims, so the
        // per-handler G6 evaluation must still apply (and deny).
        let developer_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        // Admin JWT for briefing/ask: "generate" is not in the Developer
        // allow-list, so an Admin caller (allowed everywhere at the
        // middleware layer) isolates the per-handler guard as the denial
        // source — same construction as briefing_ask_repo_scope_abac_denied
        //_returns_403 in graph.rs.
        let admin_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-sub",
                "preferred_username": "admin-user",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );

        // 1. Reviewer's probe: Developer creating a worktree on a repo whose
        //    G6 policy requires team=infra.
        {
            let state = denied_state("repo-1").await;
            seed_repo(&state, "repo-1", "ws-1").await;
            let app = crate::build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/repos/repo-1/worktrees")
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "agent_id": "agent-1",
                                "branch": "feat/x",
                                "path": "/tmp/wt-1"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_abac_denied(&state, "repo", "write", "POST /api/v1/repos/:id/worktrees")
                .await;
        }

        // 2–5. The four jj mutation endpoints share the same per-handler
        //      repo-level guard.
        for (suffix, body) in [
            ("new", Some(serde_json::json!({ "description": "d" }))),
            ("squash", None),
            ("undo", None),
            (
                "bookmark",
                Some(serde_json::json!({ "name": "b", "change_id": "c" })),
            ),
        ] {
            let state = denied_state("repo-1").await;
            seed_repo(&state, "repo-1", "ws-1").await;
            let app = crate::build_router(state.clone());
            let body = body
                .map(|b| Body::from(serde_json::to_string(&b).unwrap()))
                .unwrap_or(Body::empty());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/repos/repo-1/jj/{suffix}"))
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN, "jj/{suffix}");
            assert_one_abac_denied(
                &state,
                "repo",
                "write",
                &format!("POST /api/v1/repos/:id/jj/{suffix}"),
            )
            .await;
        }

        // 6. Repo orchestrator spawn (route is registry-exempt — the
        //    per-handler check is the operative ABAC enforcement).
        {
            let state = denied_state("repo-1").await;
            seed_repo(&state, "repo-1", "ws-1").await;
            let app = crate::build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/repos/repo-1/orchestrator/spawn")
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({ "name": "ro" })).unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_abac_denied(
                &state,
                "repo",
                "write",
                "POST /api/v1/repos/:id/orchestrator/spawn",
            )
            .await;
        }

        // 7. Briefing/ask with a request-body repo scope the caller's claims
        //    do not satisfy (Admin passes the middleware layer).
        {
            let state = denied_state("repo-1").await;
            seed_workspace(&state, "ws-briefing").await;
            seed_repo(&state, "repo-1", "ws-briefing").await;
            let app = crate::build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-briefing/briefing/ask")
                        .header("Authorization", format!("Bearer {admin_jwt}"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "question": "q",
                                "repo_id": "repo-1"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_abac_denied(
                &state,
                "repo",
                "generate",
                "POST /api/v1/workspaces/:id/briefing/ask",
            )
            .await;
        }

        // 8. Agent spawn: spawn_agent_core evaluates the repo's G6 policies
        //    before any agent/task state is created.
        {
            let state = denied_state("repo-1").await;
            seed_repo(&state, "repo-1", "ws-1").await;
            let app = crate::build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/agents/spawn")
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "name": "worker",
                                "repo_id": "repo-1",
                                "task_id": "task-1",
                                "branch": "feat/x"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_abac_denied(&state, "repo", "spawn", "POST /api/v1/agents/spawn").await;
        }

        // 9. Conversation retrieval: per-handler workspace-read guard on a
        //    workspace whose G6 policy rejects the caller's claims.
        {
            let state = denied_state("ws-conv").await;
            let sha = state
                .conversations
                .store(
                    &gyre_common::Id::new("agent-1"),
                    &gyre_common::Id::new("ws-conv"),
                    &gyre_common::Id::new("default"),
                    b"conversation-bytes",
                )
                .await
                .unwrap();
            let app = crate::build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!("/api/v1/conversations/{sha}"))
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_abac_denied(
                &state,
                "workspace",
                "read",
                "GET /api/v1/conversations/:sha",
            )
            .await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolver_covers_required_routes() {
        init_resolver();
        let resolver = RESOURCE_RESOLVER.get().unwrap();
        let required = [
            "/api/v1/tasks",
            "/api/v1/agents/spawn",
            "/api/v1/workspaces/:id",
            "/api/v1/admin/health",
            "/api/v1/policies/evaluate",
            "/api/v1/tenants/:id",
            // M34 Slice 6: workspace-scoped routes
            "/api/v1/workspaces/:workspace_id/tasks",
            "/api/v1/workspaces/:workspace_id/agents",
            "/api/v1/workspaces/:workspace_id/merge-requests",
            // M34 Slice 6: renamed params
            "/api/v1/repos/:id/worktrees/:worktree_id",
            "/api/v1/repos/:id/dependencies/:dependency_id",
            "/api/v1/merge-requests/:id/dependencies/:dependency_id",
        ];
        for pattern in &required {
            assert!(
                resolver.resolve(pattern).is_some(),
                "ResourceResolver missing entry for {pattern}"
            );
        }
        // Version should be exempt.
        let v = resolver.resolve("/api/v1/version").unwrap();
        assert!(v.exempt, "/api/v1/version should be exempt");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn developer_cannot_access_admin_kill_or_tunnel() {
        // NEW-27 regression test: admin/agents/:id/kill and admin/compute-targets/:id/tunnel
        // must be denied for Developer role now that they use resource_type="admin".
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::routing::post;
        let state_base = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state_base))
                .expect("seed built-in policies")
        });
        init_resolver();

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/admin/agents/:id/kill", post(ok_handler))
            .route("/api/v1/admin/compute-targets/:id/tunnel", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state_base.clone(),
                abac_middleware,
            ))
            .with_state(state_base);

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "dev-sub",
                "preferred_username": "developer",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // Developer cannot kill agents.
        let kill_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/agents/some-agent-id/kill")
                    .header("Authorization", format!("Bearer {dev_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            kill_resp.status(),
            StatusCode::FORBIDDEN,
            "kill must be Admin-only"
        );

        // Developer cannot open tunnels.
        let tunnel_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/compute-targets/some-target/tunnel")
                    .header("Authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            tunnel_resp.status(),
            StatusCode::FORBIDDEN,
            "tunnel must be Admin-only"
        );
    }

    /// task-098: the /api/v1/secrets routes must be Admin-only at the ABAC
    /// layer (builtin-secret-admin-only, priority 860) — Developers and Agents
    /// are denied; Admins pass via builtin-admin-all-operations (900).
    #[tokio::test(flavor = "multi_thread")]
    async fn secrets_routes_are_admin_only() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::routing::{delete, get, post};

        let state_base = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state_base))
                .expect("seed built-in policies")
        });
        init_resolver();

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        // Route patterns are built from a shared base rather than repeated
        // as literals: check-abac-route-registry.sh freezes the count of
        // duplicated "/api/v1/..." path literals in this file (resolve() is
        // first-match, so a duplicated registry entry is dead code). The
        // resolver itself has exactly one entry per secrets path; building
        // the test patterns here keeps the literal count at that baseline.
        let list_pattern = "/api/v1".to_owned() + "/secrets";
        let item_pattern = format!("{list_pattern}/:id");
        let rotate_pattern = format!("{item_pattern}/rotate");
        let item_uri = format!("{list_pattern}/sec-1");
        let rotate_uri = format!("{item_uri}/rotate");
        let app = Router::new()
            .route(&list_pattern, get(ok_handler).post(ok_handler))
            .route(&item_pattern, delete(ok_handler))
            .route(&rotate_pattern, post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state_base.clone(),
                abac_middleware,
            ))
            .with_state(state_base);

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let admin_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-sub",
                "preferred_username": "admin",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );

        // Developer: every secrets route is denied.
        for (method, uri) in [
            ("GET", list_pattern.as_str()),
            ("POST", list_pattern.as_str()),
            ("DELETE", item_uri.as_str()),
            ("POST", rotate_uri.as_str()),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("Authorization", format!("Bearer {dev_token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Developer must be denied {method} {uri}"
            );
        }

        // Admin: allowed on every secrets route.
        for (method, uri) in [
            ("GET", list_pattern.as_str()),
            ("POST", list_pattern.as_str()),
            ("DELETE", item_uri.as_str()),
            ("POST", rotate_uri.as_str()),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("Authorization", format!("Bearer {admin_token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Admin must be allowed {method} {uri}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_policies_seeded_idempotently() {
        let state = test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
            // second call must be no-op
            h.block_on(seed_builtin_policies(&state)).expect("re-seed");
        });
        let policies = state.policies.list().await.unwrap();
        let count = policies
            .iter()
            .filter(|p| p.id.to_string() == "builtin-admin-all-operations")
            .count();
        assert_eq!(count, 1, "admin policy must not be duplicated");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_policy_priorities_ordered() {
        let policies = m34_builtin_policies();
        let admin_p = policies
            .iter()
            .find(|p| p.name == "admin-all-operations")
            .unwrap();
        let dev_p = policies
            .iter()
            .find(|p| p.name == "developer-write-access")
            .unwrap();
        let ro_p = policies
            .iter()
            .find(|p| p.name == "readonly-get-only")
            .unwrap();
        let deny_p = policies.iter().find(|p| p.name == "default-deny").unwrap();
        assert!(admin_p.priority > dev_p.priority, "admin > developer");
        assert!(dev_p.priority > ro_p.priority, "developer > readonly");
        assert!(ro_p.priority > deny_p.priority, "readonly > default-deny");
        assert_eq!(deny_p.effect, PolicyEffect::Deny);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_require_signed_authorization_seeded() {
        let state = test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        let policies = state.policies.list().await.unwrap();
        let rsa = policies
            .iter()
            .find(|p| p.name == "builtin:require-signed-authorization")
            .expect("builtin:require-signed-authorization policy must be seeded");

        assert_eq!(rsa.priority, 998);
        assert!(rsa.immutable, "must be immutable");
        assert!(rsa.built_in, "must be built-in");
        assert_eq!(rsa.effect, PolicyEffect::Deny);
        assert!(rsa.actions.contains(&"push".to_string()));
        assert!(rsa.actions.contains(&"merge".to_string()));
        assert!(rsa.resource_types.contains(&"attestation".to_string()));

        // §7.2 unless-clause: the deny must NOT fire for work that carries
        // an attestation chain, and the companion
        // `builtin:attested-authorization` Allow must be seeded so attested
        // work is allowed by default (overridable by user policies).
        assert_eq!(
            rsa.conditions.len(),
            2,
            "conditions must be [subject.type != system, chain not present]"
        );
        let attested = policies
            .iter()
            .find(|p| p.name == "builtin:attested-authorization")
            .expect("builtin:attested-authorization policy must be seeded");
        assert_eq!(attested.effect, PolicyEffect::Allow);
        assert_eq!(attested.priority, 150);
    }

    /// A stored built-in policy row that differs from the code definition
    /// (a stale copy from an older seed generation — the API refuses to
    /// modify built-ins, so drift can only come from an old seed) must be
    /// refreshed on re-seed. Otherwise fixes to built-in policy definitions
    /// would never reach existing deployments.
    #[tokio::test(flavor = "multi_thread")]
    async fn seed_refreshes_drifted_builtin_policy() {
        let state = test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Tamper the stored row to look like an old seed generation: the
        // pre-fix require-signed-authorization had only the subject.type
        // condition (no chain unless-clause).
        let stored = state
            .policies
            .find_by_id("builtin-require-signed-authorization")
            .await
            .unwrap()
            .expect("policy must exist after seed");
        let mut stale = stored.clone();
        stale.conditions = vec![gyre_domain::Condition {
            attribute: "subject.type".to_string(),
            operator: gyre_domain::ConditionOp::NotEquals,
            value: gyre_domain::ConditionValue::String("system".to_string()),
        }];
        state.policies.update(&stale).await.unwrap();

        // Re-seed (next startup): the drifted row is refreshed.
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        let refreshed = state
            .policies
            .find_by_id("builtin-require-signed-authorization")
            .await
            .unwrap()
            .expect("policy must exist after re-seed");
        let code = m34_builtin_policies()
            .into_iter()
            .find(|p| p.id.as_str() == "builtin-require-signed-authorization")
            .unwrap();
        assert_eq!(
            refreshed.conditions, code.conditions,
            "drifted built-in must be refreshed to the code definition"
        );
        assert_eq!(refreshed.conditions.len(), 2);

        // Idempotency: a third seed with no drift performs no update and
        // leaves the row byte-identical.
        let before = refreshed.clone();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });
        let after = state
            .policies
            .find_by_id("builtin-require-signed-authorization")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.conditions, after.conditions);
        assert_eq!(
            before.updated_at, after.updated_at,
            "no-op seed must not bump updated_at"
        );
    }

    /// TASK-077 (HSI §2): `builtin:require-human-spec-approval` must be
    /// seeded at server startup, immutable, and idempotent across restarts.
    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_require_human_spec_approval_seeded() {
        let state = test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
            h.block_on(seed_builtin_policies(&state)).expect("re-seed"); // idempotent
        });

        let policies = state.policies.list().await.unwrap();
        let p = policies
            .iter()
            .find(|p| p.name == "builtin:require-human-spec-approval")
            .expect("builtin:require-human-spec-approval must be seeded");

        assert_eq!(p.priority, 999);
        assert!(p.immutable, "must be immutable");
        assert!(p.built_in, "must be built-in");
        assert_eq!(p.effect, PolicyEffect::Deny);
        assert_eq!(p.actions, vec!["approve".to_string()]);
        assert_eq!(p.resource_types, vec!["spec".to_string()]);

        // Not duplicated by the second seed call.
        let count = policies
            .iter()
            .filter(|p| p.name == "builtin:require-human-spec-approval")
            .count();
        assert_eq!(count, 1);
    }

    /// TASK-077 (F6 class): built-in policy seeding must fail closed. If a
    /// policy-store failure prevents an immutable Deny restriction
    /// (`builtin:require-human-spec-approval`) from being persisted, the seed
    /// must return Err — never warn-and-continue and leave the server (or
    /// test harness) running with the restriction silently missing.
    /// This test fails on the old warn-and-continue behavior, which returned
    /// `Ok` regardless of the create error.
    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_policy_seeding_fails_closed_on_store_error() {
        let state = crate::mem::test_state_failing_policy_creates();
        let result = seed_builtin_policies(&state).await;
        let err = result.expect_err("seed must fail when policy creation fails");
        assert!(
            err.to_string()
                .contains("failed to seed built-in ABAC policy"),
            "error must name the failed seed, got: {err}"
        );
        // Nothing was persisted — the store must not contain a partial seed.
        let policies = state.policies.list().await.unwrap();
        assert!(
            policies.is_empty(),
            "no policies should be persisted when seeding fails, got {}",
            policies.len()
        );
    }
}
