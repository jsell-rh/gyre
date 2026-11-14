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

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::{
    auth::AuthenticatedAgent, policy_engine, policy_engine::AttrValue,
    policy_engine::AttributeContext, AppState,
};
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

// ---------------------------------------------------------------------------
// Resource resolver
// ---------------------------------------------------------------------------

// Route patterns referenced by BOTH the resolver registry below and the
// middleware tests. check-abac-route-registry.sh greps this file for
// duplicate `/api/v1/...` string literals (dead duplicate resolver entries
// are first-match dead code), so each shared pattern is spelled exactly
// once, here, and referenced by name everywhere else.
const REPOS_ID_ROUTE: &str = "/api/v1/repos/:id";
const SPECS_ROUTE: &str = "/api/v1/specs";
const SPECS_PATH_ROUTE: &str = "/api/v1/specs/:path";
const PERSONAS_ID_ROUTE: &str = "/api/v1/personas/:id";
const REPOS_GATES_ROUTE: &str = "/api/v1/repos/:id/gates";
const REPOS_GATES_ID_ROUTE: &str = "/api/v1/repos/:id/gates/:gate_id";
const REPOS_WORKTREES_ID_ROUTE: &str = "/api/v1/repos/:id/worktrees/:worktree_id";
const REPOS_SPECS_ASSIST_ROUTE: &str = "/api/v1/repos/:id/specs/assist";
const MERGE_REQUESTS_GATES_ROUTE: &str = "/api/v1/merge-requests/:id/gates";
const MERGE_REQUESTS_DEPENDENCIES_ROUTE: &str = "/api/v1/merge-requests/:id/dependencies";
const AGENTS_MESSAGES_ROUTE: &str = "/api/v1/agents/:id/messages";
const MERGE_REQUESTS_ID_ROUTE: &str = "/api/v1/merge-requests/:id";
const MERGE_REQUESTS_COMMENTS_ROUTE: &str = "/api/v1/merge-requests/:id/comments";
const MERGE_REQUESTS_REVIEWS_ROUTE: &str = "/api/v1/merge-requests/:id/reviews";
const MERGE_REQUESTS_DIFF_ROUTE: &str = "/api/v1/merge-requests/:id/diff";
const MERGE_REQUESTS_SPEC_ROUTE: &str = "/api/v1/merge-requests/:id/spec";
const AGENTS_ID_STATUS_ROUTE: &str = "/api/v1/agents/:id/status";
const AGENTS_MESSAGES_ACK_ROUTE: &str = "/api/v1/agents/:id/messages/:message_id/ack";
/// Routes a `review:submit` gate agent may touch: read MR context, submit
/// its verdict, read comments and gate state, and fetch the pinned spec
/// (GYRE_SPEC_URL — agent-gates.md §Integration with Agent Gates step 1:
/// the gate agent receives the spec at the pinned SHA). Its ReviewRequest
/// is NOT delivered over REST: it arrives via the launch env
/// (GYRE_MR_ID / GYRE_PERSONA / …) and the /mcp surface
/// (`gyre_message_poll`, mounted outside the ABAC-layered api router),
/// which derives the inbox from the verified token subject. The
/// agent-inbox route (GET /api/v1/agents/{id}/messages) is deliberately
/// kept off this allow-list: a single-purpose review token has no need
/// for it (least privilege / defense in depth).
const REVIEWER_ALLOWED_ROUTES: &[&str] = &[
    MERGE_REQUESTS_ID_ROUTE,
    MERGE_REQUESTS_REVIEWS_ROUTE,
    MERGE_REQUESTS_DIFF_ROUTE,
    MERGE_REQUESTS_COMMENTS_ROUTE,
    MERGE_REQUESTS_GATES_ROUTE,
    MERGE_REQUESTS_SPEC_ROUTE,
    "/api/v1/version",
];

/// Routes a `validation:report` gate agent may touch: read-only MR context
/// and the pinned spec for its check and nothing else (its contract is the
/// process exit code).
const VALIDATOR_ALLOWED_ROUTES: &[&str] = &[
    MERGE_REQUESTS_ID_ROUTE,
    MERGE_REQUESTS_DIFF_ROUTE,
    MERGE_REQUESTS_SPEC_ROUTE,
    "/api/v1/version",
];

/// A single entry in the route registry: maps an axum route pattern to a
/// resource type and optional path parameters for workspace resolution.
pub struct RouteResourceMapping {
    /// Axum route pattern, e.g. "/api/v1/tasks/:id".
    pub pattern: &'static str,
    /// The ABAC resource type string, e.g. "task".
    pub resource_type: &'static str,
    /// Override the action for this route (e.g. "spawn", "approve").
    pub action_override: Option<&'static str>,
    /// Per-method action overrides, consulted before `action_override`.
    /// Needed when one pattern serves multiple methods that need distinct
    /// ABAC actions (e.g. PUT = "transfer_ownership" but DELETE = "delete"
    /// on the workspace member route).
    pub method_action_overrides: &'static [(&'static str, &'static str)],
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
            method_action_overrides: &[],
            exempt: false,
        }
    }

    const fn api_with_method_actions(
        pattern: &'static str,
        resource_type: &'static str,
        method_action_overrides: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            pattern,
            resource_type,
            action_override: None,
            method_action_overrides,
            exempt: false,
        }
    }

    const fn exempt(pattern: &'static str) -> Self {
        Self {
            pattern,
            resource_type: "exempt",
            action_override: None,
            method_action_overrides: &[],
            exempt: true,
        }
    }

    /// Resolve the ABAC action for a request on this route: method-scoped
    /// overrides first, then the pattern-wide override, then the HTTP
    /// method mapping.
    fn resolve_action(&self, method: &Method) -> &'static str {
        if let Some((_, action)) = self
            .method_action_overrides
            .iter()
            .find(|(m, _)| m.eq_ignore_ascii_case(method.as_str()))
        {
            return action;
        }
        self.action_override
            .unwrap_or_else(|| method_to_action(method))
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
                RouteResourceMapping::api(REPOS_ID_ROUTE, "repo", None),
                // Repo access overrides (user-management.md §Repo Access).
                // PUT/DELETE on the override route evaluate as the repo
                // "admin" action (override management is an admin
                // capability); GET keeps its method-derived action.
                RouteResourceMapping::api_with_method_actions(
                    "/api/v1/repos/:id/access-overrides/:user_id",
                    "repo",
                    &[("PUT", "admin"), ("DELETE", "admin")],
                ),
                RouteResourceMapping::api("/api/v1/repos/:id/access-overrides", "repo", None),
                RouteResourceMapping::api("/api/v1/repos/:id/access/:user_id", "repo", None),
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
                RouteResourceMapping::api(REPOS_WORKTREES_ID_ROUTE, "worktree", None),
                RouteResourceMapping::api(REPOS_GATES_ROUTE, "gate", None),
                RouteResourceMapping::api(REPOS_GATES_ID_ROUTE, "gate", None),
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
                RouteResourceMapping::api("/api/v1/repos/:id/graph/concepts", "graph", None),
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
                RouteResourceMapping::api(AGENTS_ID_STATUS_ROUTE, "agent", Some("write")),
                RouteResourceMapping::api("/api/v1/agents/:id/heartbeat", "agent", Some("write")),
                RouteResourceMapping::api("/api/v1/agents/:id/complete", "agent", Some("complete")),
                RouteResourceMapping::api("/api/v1/agents/:id/card", "agent", Some("write")),
                RouteResourceMapping::api(AGENTS_MESSAGES_ROUTE, "message", None),
                RouteResourceMapping::api(AGENTS_MESSAGES_ACK_ROUTE, "message", Some("write")),
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
                RouteResourceMapping::api(MERGE_REQUESTS_ID_ROUTE, "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/status",
                    "merge_request",
                    Some("write"),
                ),
                RouteResourceMapping::api(MERGE_REQUESTS_COMMENTS_ROUTE, "merge_request", None),
                RouteResourceMapping::api(MERGE_REQUESTS_REVIEWS_ROUTE, "merge_request", None),
                RouteResourceMapping::api(MERGE_REQUESTS_DIFF_ROUTE, "merge_request", None),
                RouteResourceMapping::api(MERGE_REQUESTS_SPEC_ROUTE, "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/attestation",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(MERGE_REQUESTS_GATES_ROUTE, "gate", None),
                // MR SDLC timeline (S2.5 — HSI §3)
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/timeline",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(MERGE_REQUESTS_DEPENDENCIES_ROUTE, "dependency", None),
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
                RouteResourceMapping::api(SPECS_ROUTE, "spec", None),
                RouteResourceMapping::api("/api/v1/specs/pending", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/drifted", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/index", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/graph", "spec", None),
                RouteResourceMapping::api(SPECS_PATH_ROUTE, "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/approve", "spec", Some("approve")),
                RouteResourceMapping::api("/api/v1/specs/:path/revoke", "spec", Some("write")),
                RouteResourceMapping::api("/api/v1/specs/:path/history", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/links", "spec", None),
                RouteResourceMapping::api("/api/v1/specs/:path/progress", "spec", None),
                // task-204: spec-lifecycle accountability patrol. Registered here
                // (not in scripts/abac-route-registry-exemptions.txt, which is
                // frozen) so the route gets real policy evaluation. It mutates
                // state — every finding is persisted as an orchestrator
                // escalation — and needs the same write-level policy as the other
                // spec-write routes; POST derives the `write` action
                // (`method_to_action`), so no `action_override` is needed.
                RouteResourceMapping::api("/api/v1/patrol/spec-lifecycle", "spec", None),
                // ── Spec editing backend (S3.3) ────────────────────────────
                RouteResourceMapping::api(REPOS_SPECS_ASSIST_ROUTE, "spec", Some("generate")),
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
                RouteResourceMapping::api("/api/v1/analytics/decide", "analytics", None),
                RouteResourceMapping::api("/api/v1/analytics/decide/custom", "analytics", None),
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
                RouteResourceMapping::api("/api/v1/admin/orphaned-workspaces", "admin", None),
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
                RouteResourceMapping::api("/api/v1/workspaces/:id/budget/by-team", "budget", None),
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
                RouteResourceMapping::api("/api/v1/tenants/:id/meta-specs/set", "meta_spec", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/meta-specs/rollout-policy",
                    "meta_spec",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/meta-specs/assist",
                    "meta_spec",
                    Some("generate"),
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/members",
                    "workspace_member",
                    None,
                ),
                // PUT (role change incl. ownership transfer) evaluates as the
                // "transfer_ownership" action (user-management.md §Ownership
                // Transfer & Reclamation); DELETE keeps "delete".
                RouteResourceMapping::api_with_method_actions(
                    "/api/v1/workspaces/:id/members/:user_id",
                    "workspace_member",
                    &[("PUT", "transfer_ownership")],
                ),
                // Spec-named alias route (user-management.md §API:
                // `PUT .../members/{user_id}/role`) — mirrors the member
                // route's action resolution so both paths enforce the same
                // policy evaluation.
                RouteResourceMapping::api_with_method_actions(
                    "/api/v1/workspaces/:id/members/:user_id/role",
                    "workspace_member",
                    &[("PUT", "transfer_ownership")],
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/teams", "team", None),
                RouteResourceMapping::api("/api/v1/workspaces/:id/teams/:team_id", "team", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/teams/:team_id/members",
                    "team",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/teams/:team_id/members/:user_id",
                    "team",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/graph", "graph", None),
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/graph/concept/:concept_name",
                    "graph",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/workspaces/:id/graph/concepts", "graph", None),
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
                // Conversational exploration Q&A (system-explorer.md §8) —
                // an LLM endpoint like /generate: same resource, same
                // `generate` action (Developer+ workspace role via the
                // builtin-developer-generate-access policy). Registered
                // BEFORE /:view_id — the resolver is first-match and
                // "/ask" would otherwise be captured as a view_id param.
                RouteResourceMapping::api(
                    "/api/v1/workspaces/:id/explorer-views/ask",
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
                RouteResourceMapping::api(PERSONAS_ID_ROUTE, "persona", None),
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
                RouteResourceMapping::exempt("/api/v1/notifications"),
                RouteResourceMapping::exempt("/api/v1/notifications/unread-count"),
                RouteResourceMapping::exempt("/api/v1/notifications/mark-all-read"),
                RouteResourceMapping::exempt("/api/v1/notifications/preferences"),
                RouteResourceMapping::exempt("/api/v1/notifications/stream"),
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
// Path attribute extraction
// ---------------------------------------------------------------------------

/// Zip a route pattern against a concrete request path and collect the
/// `:param` → value bindings. Segment values are taken verbatim (no
/// percent-decoding); registry route params are plain slugs/ids.
fn path_params(pattern: &str, path: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (p, v) in pattern.split('/').zip(path.split('/')) {
        if let Some(name) = p.strip_prefix(':') {
            out.insert(name.to_string(), v.to_string());
        }
    }
    out
}

/// Path prefix for workspace-scoped routes; shared by the workspace-id,
/// repo-id and resource-id extraction branches below so the literal is
/// spelled once (the ABAC registry check flags duplicate `/api/v1/...`
/// literals).
const WORKSPACES_PATH_PREFIX: &str = "/api/v1/workspaces/";

/// Path prefix for repo-scoped routes; see [`WORKSPACES_PATH_PREFIX`].
const REPOS_PATH_PREFIX: &str = "/api/v1/repos/";

/// Entity type named by the `:id` param on a parent-scoped route prefix.
///
/// A parent-scoped prefix is `/api/v1/<collection>/...` where the leading
/// `<collection>` segment (plural) names the entity its `:id` addresses:
/// `workspaces` → `workspace`, `repos` → `repo`, `merge-requests` →
/// `merge_request`, `agents` → `agent`. Singularization is mechanical
/// (plural `-ies` → `-y`, else strip the plural `-s`; hyphens are the URL
/// form of underscores) except for compound collection names that name a
/// registry OF another entity type (`meta-specs-registry` addresses
/// `meta_spec` entities, so its `:id` is a meta_spec id, not a
/// "meta_spec_registry" id).
///
/// Returns `None` when the pattern is not a `/api/v1/<collection>/...`
/// route (missing or param-valued collection segment) — such routes carry
/// no prefix entity, so `:id` is never treated as a parent id by virtue of
/// the prefix alone.
fn prefix_entity_type(pattern: &str) -> Option<String> {
    let rest = pattern.strip_prefix("/api/v1/")?;
    let collection = rest.split('/').next()?;
    if collection.is_empty() || collection.starts_with(':') {
        return None;
    }
    if collection == "meta-specs-registry" {
        return Some("meta_spec".to_string());
    }
    let singular = if let Some(stem) = collection.strip_suffix("ies") {
        format!("{stem}y")
    } else {
        collection
            .strip_suffix('s')
            .unwrap_or(collection)
            .to_string()
    };
    Some(singular.replace('-', "_"))
}

/// True when the route pattern carries path segments AFTER the `:id`
/// segment — i.e. the route addresses a sub-collection or sub-resource of
/// the `:id` entity (`/repos/:id/gates`), rather than the `:id` entity
/// itself (`/repos/:id`).
fn has_child_segments_after_id(pattern: &str) -> bool {
    let mut segments = pattern.split('/');
    segments.any(|s| s == ":id") && segments.next().is_some()
}

/// Extract resource identity attributes from the request path into the
/// attribute context (§Attributes: `resource.id`, `resource.workspace_id`,
/// `resource.repo_id` — source "Request path").
///
/// Path-parameter conventions in the route registry:
/// - `:workspace_id` names the workspace explicitly wherever it appears.
/// - On parent-scoped routes (`/api/v1/workspaces/:id/...`,
///   `/api/v1/repos/:id/...`) the `:id` names the PARENT entity, exposed as
///   `resource.workspace_id` / `resource.repo_id` respectively.
/// - A param named `{resource_type}_id` (`:worktree_id` on a "worktree"
///   route, `:gate_id` on a "gate" route, `:team_id` on a "team" route)
///   names the addressed entity wherever it appears and takes precedence
///   over `:id`.
/// - Elsewhere `:id` names the route's own entity → `resource.id`.
///   On parent-scoped child routes (e.g. `/workspaces/:id/tasks`,
///   `/repos/:id/gates`, `/merge-requests/:id/gates`,
///   `/agents/:id/messages` — the route carries segments beyond the `:id`
///   AND its resource type ≠ the prefix collection's entity type) the `:id`
///   is the parent, not the addressed entity, so no `resource.id` is
///   fabricated from it; the addressed entity surfaces via its
///   `{resource_type}_id` param when the route names one (e.g.
///   `/repos/:id/worktrees/:worktree_id`), else it stays unset and
///   conditions on it fail closed.
/// - A `:path` param names the addressed entity by its registry path,
///   URL-encoded (spec and meta-spec routes: `/specs/:path`,
///   `/repos/:id/specs/:path/assertions`, `/meta-specs/:path/blast-radius`).
///   The percent-DECODED path is the resource identity (§Attributes:
///   `resource.id`, source "Request path", example `system/identity-security.md`);
///   invalid percent-encoding leaves `resource.id` unset (fail closed).
///
/// Attributes requiring entity lookup (`resource.tenant_id`, `owner`, `team`,
/// `approval_status`, `visibility`) are populated by
/// [`extract_entity_attributes`] below. Nested child ids like `:node_id` are
/// not resource identity attributes and stay unpopulated.
fn extract_path_attributes(
    ctx: &mut AttributeContext,
    pattern: &str,
    path: &str,
    resource_type: &str,
) {
    let params = path_params(pattern, path);

    let workspace_id = params.get("workspace_id").cloned().or_else(|| {
        pattern
            .starts_with(WORKSPACES_PATH_PREFIX)
            .then(|| params.get("id").cloned())
            .flatten()
    });
    if let Some(ws) = workspace_id {
        ctx.set("resource.workspace_id", ws);
    }

    let repo_id = params.get("repo_id").cloned().or_else(|| {
        pattern
            .starts_with(REPOS_PATH_PREFIX)
            .then(|| params.get("id").cloned())
            .flatten()
    });
    if let Some(repo) = repo_id {
        ctx.set("resource.repo_id", repo);
    }

    // A param named `{resource_type}_id` names the addressed entity wherever
    // it appears — including on parent-scoped child routes, where the `:id`
    // names the parent instead (e.g. `:worktree_id` on
    // `/repos/:id/worktrees/:worktree_id`, `:gate_id` on
    // `/repos/:id/gates/:gate_id`, `:dependency_id` on
    // `/merge-requests/:id/dependencies/:dependency_id`).
    let child_param = format!("{resource_type}_id");
    let resource_id = params.get(&child_param).cloned().or_else(|| {
        // On parent-scoped child routes (`/workspaces/:id/tasks`,
        // `/repos/:id/gates`, `/merge-requests/:id/gates`,
        // `/agents/:id/messages`, ...) the `:id` names the PARENT entity —
        // already exposed as resource.workspace_id / resource.repo_id above
        // where applicable — not the addressed entity, so it must not leak
        // into resource.id. A route is parent-scoped when it carries path
        // segments beyond the `:id` AND its registered resource type
        // differs from the entity type the prefix collection names
        // ([`prefix_entity_type`]). The classification is derived from the
        // pattern itself, so any prefix's child routes are covered — the
        // round-4 review finding flagged merge-requests/agents routes
        // leaking the parent MR/agent id precisely because an earlier fix
        // hardcoded only the workspaces/repos prefixes.
        if has_child_segments_after_id(pattern)
            && prefix_entity_type(pattern).is_some_and(|entity| entity != resource_type)
        {
            None
        } else {
            params.get("id").cloned()
        }
    });
    if let Some(id) = resource_id {
        ctx.set("resource.id", id);
    }

    // A `:path` param names the addressed entity by its registry path,
    // URL-encoded (spec/meta-spec routes — the spec's own §Attributes
    // example for resource.id is a spec path, `system/identity-security.md`).
    // The DECODED path is the identity; a value that is not valid
    // percent-encoding cannot name an entity, so `resource.id` stays unset
    // and conditions on it fail closed.
    if let Some(raw) = params.get("path") {
        match percent_decode(raw) {
            Some(decoded) => ctx.set("resource.id", decoded),
            None => tracing::warn!(
                %raw,
                "path param is not valid percent-encoding; resource.id unset"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Entity-lookup resource attributes
// ---------------------------------------------------------------------------

/// Percent-decode a path parameter (spec routes carry `system%2Ffoo.md`).
/// Returns `None` when the value contains invalid escapes — a malformed
/// identity cannot be looked up, and conditions on the looked-up attributes
/// fail closed.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = (bytes[i + 1] as char).to_digit(16)?;
            let lo = (bytes[i + 2] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Read a `Single` string attribute from the context.
fn attr_str<'a>(ctx: &'a AttributeContext, key: &str) -> Option<&'a str> {
    match ctx.get(key) {
        Some(AttrValue::Single(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Set `resource.tenant_id` from the addressed workspace's tenant. Absent
/// when the workspace does not exist — no tenant is fabricated.
async fn set_tenant_from_workspace(
    state: &AppState,
    ctx: &mut AttributeContext,
    workspace_id: &str,
) {
    if ctx.has("resource.tenant_id") {
        return;
    }
    match state
        .workspaces
        .find_by_id(&gyre_common::Id::new(workspace_id))
        .await
    {
        Ok(Some(ws)) => ctx.set("resource.tenant_id", ws.tenant_id.to_string()),
        Ok(None) => {}
        Err(e) => tracing::warn!(
            workspace_id = %workspace_id,
            err = %e,
            "workspace lookup for resource.tenant_id failed; attribute unset"
        ),
    }
}

/// Set `resource.tenant_id` from the addressed repo's workspace tenant.
async fn set_tenant_from_repo(state: &AppState, ctx: &mut AttributeContext, repo_id: &str) {
    if ctx.has("resource.tenant_id") {
        return;
    }
    match state.repos.find_by_id(&gyre_common::Id::new(repo_id)).await {
        Ok(Some(repo)) => {
            set_tenant_from_workspace(state, ctx, &repo.workspace_id.to_string()).await
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(
            repo_id = %repo_id,
            err = %e,
            "repo lookup for resource.tenant_id failed; attribute unset"
        ),
    }
}

/// Complete the scope chain for a repo-addressed route: set
/// `resource.workspace_id` from the addressed repo's own workspace (the
/// path names none), then `resource.tenant_id` from that workspace.
/// Absent when the repo does not exist — no scope is fabricated.
async fn set_workspace_and_tenant_from_repo(
    state: &AppState,
    ctx: &mut AttributeContext,
    repo_id: &str,
) {
    match state.repos.find_by_id(&gyre_common::Id::new(repo_id)).await {
        Ok(Some(repo)) => {
            if !ctx.has("resource.workspace_id") {
                ctx.set("resource.workspace_id", repo.workspace_id.to_string());
            }
            set_tenant_from_workspace(state, ctx, &repo.workspace_id.to_string()).await;
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(
            repo_id = %repo_id,
            err = %e,
            "repo lookup for scope-chain resolution failed; attributes unset"
        ),
    }
}

/// Extract resource attributes sourced from entity lookup / entity fields
/// (§Attributes: `resource.tenant_id`, `resource.owner`, `resource.team`,
/// `resource.approval_status`, `resource.visibility`).
///
/// Looks up the addressed entity in the real stores and copies its scope and
/// ownership fields into the context. Lookup failures and missing entities
/// leave the attributes unset — conditions on them then fail closed (a
/// missing attribute never matches), the same discipline as the
/// membership-sourced subject attributes.
///
/// `resource.visibility` and `resource.team` apply only to entities that
/// carry those fields (none today: specs and personas carry `owner` +
/// `approval_status`; scope-scoped entities carry their workspace/repo/tenant
/// identity). When an entity type gains those fields, its arm here must set
/// them.
async fn extract_entity_attributes(
    state: &AppState,
    ctx: &mut AttributeContext,
    pattern: &str,
    path: &str,
    resource_type: &str,
) {
    let params = path_params(pattern, path);
    let resource_id = attr_str(ctx, "resource.id").map(|s| s.to_string());

    // Resolve the tenant of the addressed scope chain: repo → workspace →
    // tenant. Runs for every resource type: repo/workspace-parented routes
    // name their parent directly; entity lookups below backfill via the
    // entity's own workspace.
    //
    // A repo-addressed route names no workspace in the path, so the repo's
    // own workspace completes the chain (user-management.md §Repo Access:
    // repo access is inherited from workspace membership, so
    // membership-keyed attributes like `subject.workspace_role` must
    // resolve against the repo's workspace for the builtin
    // repo-access-override-management policy to govern the repo `admin`
    // action). An unresolvable repo id leaves the attributes unset —
    // conditions on them fail closed, never the reverse.
    let path_repo_id = attr_str(ctx, "resource.repo_id").map(|s| s.to_string());
    let path_workspace_id = attr_str(ctx, "resource.workspace_id").map(|s| s.to_string());
    if let Some(repo_id) = path_repo_id {
        set_workspace_and_tenant_from_repo(state, ctx, &repo_id).await;
    } else if let Some(ws_id) = path_workspace_id {
        set_tenant_from_workspace(state, ctx, &ws_id).await;
    }

    match resource_type {
        // Spec routes address the spec by its registry path (`:path`,
        // URL-encoded). The ledger entry carries owner, approval status,
        // and the repo/workspace scope (§Attributes: "Entity lookup",
        // "Entity field").
        "spec" => {
            let Some(raw) = params.get("path") else {
                return;
            };
            let Some(spec_path) = percent_decode(raw) else {
                tracing::warn!(%raw, "spec path is not valid percent-encoding; entity attributes unset");
                return;
            };
            match state.spec_ledger.find_by_path(&spec_path).await {
                Ok(Some(entry)) => {
                    ctx.set("resource.owner", entry.owner.clone());
                    ctx.set(
                        "resource.approval_status",
                        entry.approval_status.to_string(),
                    );
                    if let Some(repo) = &entry.repo_id {
                        if !ctx.has("resource.repo_id") {
                            ctx.set("resource.repo_id", repo.clone());
                        }
                        set_tenant_from_repo(state, ctx, repo).await;
                    }
                    if let Some(ws) = &entry.workspace_id {
                        if !ctx.has("resource.workspace_id") {
                            ctx.set("resource.workspace_id", ws.clone());
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    spec_path = %spec_path,
                    err = %e,
                    "spec ledger lookup failed; entity attributes unset"
                ),
            }
        }
        // Persona: owner and approval lifecycle (§Attributes "Entity field").
        "persona" => {
            let Some(id) = resource_id else { return };
            match state
                .personas
                .find_by_id(&gyre_common::Id::new(id.clone()))
                .await
            {
                Ok(Some(p)) => {
                    if let Some(owner) = &p.owner {
                        ctx.set("resource.owner", owner.clone());
                    }
                    ctx.set(
                        "resource.approval_status",
                        match p.approval_status {
                            gyre_domain::PersonaApprovalStatus::Pending => "pending",
                            gyre_domain::PersonaApprovalStatus::Approved => "approved",
                            gyre_domain::PersonaApprovalStatus::Deprecated => "deprecated",
                        },
                    );
                    match &p.scope {
                        gyre_domain::PersonaScope::Workspace(ws) => {
                            if !ctx.has("resource.workspace_id") {
                                ctx.set("resource.workspace_id", ws.to_string());
                            }
                            set_tenant_from_workspace(state, ctx, &ws.to_string()).await;
                        }
                        gyre_domain::PersonaScope::Tenant(t) => {
                            if !ctx.has("resource.tenant_id") {
                                ctx.set("resource.tenant_id", t.to_string());
                            }
                        }
                        gyre_domain::PersonaScope::Repo(r) => {
                            if !ctx.has("resource.repo_id") {
                                ctx.set("resource.repo_id", r.to_string());
                            }
                            set_tenant_from_repo(state, ctx, &r.to_string()).await;
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    persona_id = %id,
                    err = %e,
                    "persona lookup failed; entity attributes unset"
                ),
            }
        }
        // Task: workspace/repo scope from the entity itself.
        "task" => {
            let Some(id) = resource_id else { return };
            match state
                .tasks
                .find_by_id(&gyre_common::Id::new(id.clone()))
                .await
            {
                Ok(Some(t)) => {
                    if !ctx.has("resource.workspace_id") {
                        ctx.set("resource.workspace_id", t.workspace_id.to_string());
                    }
                    if !ctx.has("resource.repo_id") {
                        ctx.set("resource.repo_id", t.repo_id.to_string());
                    }
                    set_tenant_from_workspace(state, ctx, &t.workspace_id.to_string()).await;
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    task_id = %id,
                    err = %e,
                    "task lookup failed; entity attributes unset"
                ),
            }
        }
        // Agent: workspace/repo scope from the entity itself.
        "agent" => {
            let Some(id) = resource_id else { return };
            match state
                .agents
                .find_by_id(&gyre_common::Id::new(id.clone()))
                .await
            {
                Ok(Some(a)) => {
                    if !ctx.has("resource.workspace_id") {
                        ctx.set("resource.workspace_id", a.workspace_id.to_string());
                    }
                    if !ctx.has("resource.repo_id") {
                        if let Some(r) = &a.repo_id {
                            ctx.set("resource.repo_id", r.to_string());
                        }
                    }
                    set_tenant_from_workspace(state, ctx, &a.workspace_id.to_string()).await;
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    agent_id = %id,
                    err = %e,
                    "agent lookup failed; entity attributes unset"
                ),
            }
        }
        // Merge request: repo via repository_id, workspace from the entity.
        "merge_request" => {
            let Some(id) = resource_id else { return };
            match state
                .merge_requests
                .find_by_id(&gyre_common::Id::new(id.clone()))
                .await
            {
                Ok(Some(mr)) => {
                    if !ctx.has("resource.workspace_id") {
                        ctx.set("resource.workspace_id", mr.workspace_id.to_string());
                    }
                    if !ctx.has("resource.repo_id") {
                        ctx.set("resource.repo_id", mr.repository_id.to_string());
                    }
                    set_tenant_from_repo(state, ctx, &mr.repository_id.to_string()).await;
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    mr_id = %id,
                    err = %e,
                    "merge request lookup failed; entity attributes unset"
                ),
            }
        }
        // Team routes name the team directly (`:team_id`) or via `:id`
        // under a workspace parent; both carry the owning workspace.
        "team" => {
            let team_param = params
                .get("team_id")
                .cloned()
                .or_else(|| resource_id.clone());
            let Some(team_id) = team_param else { return };
            match state
                .teams
                .find_by_id(&gyre_common::Id::new(team_id.clone()))
                .await
            {
                Ok(Some(team)) => {
                    if !ctx.has("resource.workspace_id") {
                        ctx.set("resource.workspace_id", team.workspace_id.to_string());
                    }
                    set_tenant_from_workspace(state, ctx, &team.workspace_id.to_string()).await;
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    team_id = %team_id,
                    err = %e,
                    "team lookup failed; entity attributes unset"
                ),
            }
        }
        // Tenant-scoped entities need no further lookup: the path names the
        // tenant (registry: `/api/v1/tenants/:id`), and the tenant of a
        // tenant is itself.
        _ => {}
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
    let by = Id::new("system");

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
        // Priority 810: workspace Owner/Admin members manage workspace
        // membership (user-management.md §Workspace Membership: Admin —
        // "Manage members"; Owner — full workspace control). Gates the
        // "transfer_ownership" action (role changes, including ownership
        // transfer / TenantAdmin reassignment) and member removal on the
        // workspace_member resource. `subject.workspace_role` is resolved
        // from the real membership store by the middleware; callers without
        // a membership don't match and fall through to the default-deny.
        Policy {
            id: Id::new("builtin-workspace-member-management"),
            name: "builtin:workspace-member-management".to_string(),
            description: "Workspace Owner/Admin members may manage workspace membership \
                 (role changes, ownership transfer, member removal)"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 810,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::In,
                value: ConditionValue::StringList(vec![
                    "Owner".to_string(),
                    "Admin".to_string(),
                ]),
            }],
            actions: vec!["transfer_ownership".to_string(), "delete".to_string()],
            resource_types: vec!["workspace_member".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Priority 809: workspace Owner/Admin members manage per-repo access
        // overrides (user-management.md §Repo Access: the Admin role carries
        // repo `admin`). Gates the repo "admin" action, which the override
        // management route resolves to; no other repo route produces that
        // action, so this does not broaden repo deletion/update.
        Policy {
            id: Id::new("builtin-repo-access-override-management"),
            name: "builtin:repo-access-override-management".to_string(),
            description: "Workspace Owner/Admin members may manage per-repo access overrides"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 809,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::In,
                value: ConditionValue::StringList(vec![
                    "Owner".to_string(),
                    "Admin".to_string(),
                ]),
            }],
            actions: vec!["admin".to_string()],
            resource_types: vec!["repo".to_string()],
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

        // Priority 800: Developer workspace role → allow LLM generation
        // (`generate` action) on explorer_view / spec / workspace resources.
        //
        // HSI §1444-1445 (spec-amendment table): `generate` is distinct from
        // `write` so policies can permit LLM generation while restricting
        // CRUD. The five LLM endpoints (`explorer-views/generate`,
        // `explorer-views/ask`, `briefing/ask`, `specs/assist`,
        // `prompts/save`) use
        // `action_override: "generate"`, which no other builtin Allow
        // covers — without this policy, `default-deny` 403s every LLM call
        // for non-Admin users.
        //
        // Keyed on `subject.workspace_role` (sourced from the membership
        // store — never from JWT claims, which are caller-chosen content),
        // not the global role: LLM generation is a workspace-scoped capability
        // (HSI §1325 "Requires workspace membership"), so a user with a
        // global Developer role but Viewer workspace role must be denied.
        // `briefing/ask` maps to resource_type `workspace` (HSI §1325),
        // so `workspace` must be covered alongside the spec's
        // `explorer_view` and `spec` (hierarchy-enforcement.md built-ins
        // table).
        Policy {
            id: Id::new("builtin-developer-generate-access"),
            name: "developer-generate-access".to_string(),
            description: "Developer and Admin workspace roles can perform the generate action on LLM endpoints".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 800,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::In,
                value: ConditionValue::StringList(vec![
                    "Owner".to_string(),
                    "Admin".to_string(),
                    "Developer".to_string(),
                ]),
            }],
            actions: vec!["generate".to_string()],
            resource_types: vec![
                "explorer_view".to_string(),
                "spec".to_string(),
                "workspace".to_string(),
            ],
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

/// Map agent-JWT claim names onto the spec-named `subject.*` attributes
/// (§Attributes) where they differ.
///
/// - `wl_stack_hash` (G10 workload claim) → `subject.stack_hash`
/// - `repo_id` (spawn scope) → `subject.repo_scope`
/// - `attestation_level` (numeric claim) → `subject.attestation_level` (number)
///
/// Called only for agent-type subjects — the middleware gates the whole
/// claim-merge block (allowlisted `merge_agent_jwt_claims` + this
/// normalization) on `subject.type == "agent"`. §Attributes sources these
/// names from the "Agent OIDC claim", so a HUMAN token carrying them must
/// not define them either.
///
/// Only sets an attribute when the claim is present and non-empty; a missing
/// claim leaves the attribute unset so conditions on it fail closed. Never
/// overwrites an attribute already present in the context (guarded below
/// with `contains_key`, matching the insert-only `merge_agent_jwt_claims`
/// that runs immediately before it on the same claims object).
fn normalize_agent_jwt_attributes(ctx: &mut AttributeContext, claims: &serde_json::Value) {
    let obj = match claims.as_object() {
        Some(o) => o,
        None => return,
    };
    if let Some(hash) = obj.get("wl_stack_hash").and_then(|v| v.as_str()) {
        if !hash.is_empty() && !ctx.has("subject.stack_hash") {
            ctx.set("subject.stack_hash", hash);
        }
    }
    if let Some(repo) = obj.get("repo_id").and_then(|v| v.as_str()) {
        if !repo.is_empty() && !ctx.has("subject.repo_scope") {
            ctx.set("subject.repo_scope", repo);
        }
    }
    if let Some(level) = obj.get("attestation_level").and_then(|v| v.as_i64()) {
        if !ctx.has("subject.attestation_level") {
            ctx.set_number("subject.attestation_level", level);
        }
    }
}

/// Remaining tokens in a workspace's daily budget (§Attributes:
/// `env.budget_remaining`, source "Budget system").
///
/// `None` when no per-day token cap is configured — the budget system imposes
/// no bound, so no value is fabricated (a policy conditioning on the
/// attribute fails closed instead of matching an invented number).
fn budget_tokens_remaining(
    config: Option<&gyre_domain::BudgetConfig>,
    usage: Option<&gyre_domain::BudgetUsage>,
) -> Option<i64> {
    let max = config?.max_tokens_per_day?;
    let used = usage.map(|u| u.tokens_used_today).unwrap_or(0);
    Some(max.saturating_sub(used) as i64)
}

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
    let mapping = RESOURCE_RESOLVER.get().and_then(|r| r.resolve(&pattern));
    let (resource_type, exempt) = match mapping {
        Some(m) => (m.resource_type, m.exempt),
        None => {
            // No policy evaluation, but the call is still attributed
            // when made under an impersonation token
            // (identity-security.md §Impersonation).
            let req = record_impersonated_passthrough(state.clone(), req, "abac-unresolved").await;
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
    // policy evaluation): method-scoped overrides, then pattern-wide
    // override, then the HTTP method mapping.
    let action = match mapping {
        Some(m) => m.resolve_action(&method),
        None => method_to_action(&method),
    };

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
    // (review F3). §1 Token Scoping: the scope claim is a list; read it
    // through the extractor-derived `auth.scope` (mirrored from
    // signature-verified claims), never the raw JSON string form — an
    // array claim read as a string silently fails open.
    let is_reviewer = auth
        .scope
        .iter()
        .any(|s| s == crate::auth::REVIEW_SUBMIT_SCOPE);
    let is_validator = auth
        .scope
        .iter()
        .any(|s| s == crate::auth::VALIDATION_REPORT_SCOPE);
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
                scope = ?auth.scope,
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

    // Build attribute context (§Attributes).
    let mut ctx = AttributeContext::default();

    let subject_type = if auth.roles.contains(&gyre_domain::UserRole::Agent) {
        "agent"
    } else {
        "user"
    };
    ctx.set("subject.type", subject_type);
    // Subject identity from the auth context: the authenticated identity
    // string (JWT display name, agent-token id, or API-key subject).
    ctx.set("subject.id", &auth.agent_id);

    let global_role = auth.roles.first().map(|r| r.as_str()).unwrap_or("ReadOnly");
    ctx.set("subject.global_role", global_role);
    // Tenant identity from the auth context (§Attributes: subject.tenant_id,
    // source "Auth context / OIDC"). Set for every auth path — agent and
    // API-key callers do not carry JWT claims, so they get tenant_id only
    // from here.
    ctx.set("subject.tenant_id", &auth.tenant_id);
    // OIDC-claim-sourced subject attributes (§Attributes: persona, stack_hash,
    // attestation_level, repo_scope — source "Agent OIDC claim"), for AGENT
    // subjects only. Claim merging is allowlisted to those four names
    // (merge_agent_jwt_claims) and gated on subject.type == "agent": every
    // other subject attribute has a canonical non-claim source — the auth
    // extractor for type/id/global_role/tenant_id, the membership and team
    // stores for workspace_role/workspace_ids/team_ids — and a verified
    // JWT's claim CONTENT is caller-chosen, so a raw claim carrying e.g.
    // {"workspace_role": "Admin"} must never define the attribute. Before
    // the gate + allowlist (review round 2) such a claim defined the
    // attribute on any request whose membership block did not overwrite it
    // (membership-less caller, unaddressed workspace, repo-parented route)
    // and satisfied membership-keyed Allow policies; the same channel let
    // human tokens define persona/attestation_level (spec source: agent
    // claim). The spec-named normalization maps the agent-JWT
    // `wl_stack_hash` claim onto the spec's `stack_hash` attribute name.
    if subject_type == "agent" {
        if let Some(claims) = &auth.jwt_claims {
            ctx.merge_agent_jwt_claims(claims);
            normalize_agent_jwt_attributes(&mut ctx, claims);
        }
    }

    // Resource identity attributes from the request path (§Attributes:
    // resource.id / resource.workspace_id / resource.repo_id, source
    // "Request path"). Runs BEFORE membership extraction: the membership
    // block reads `resource.workspace_id` to select the caller's role in the
    // addressed workspace. `resource.type`, `action`, and `env.time` are
    // injected by the evaluation engine itself.
    extract_path_attributes(&mut ctx, &pattern, req.uri().path(), resource_type);

    // Resource attributes from entity lookup / entity fields (§Attributes:
    // resource.tenant_id, owner, team, approval_status, visibility — source
    // "Entity lookup" / "Entity field"). Runs after path extraction so the
    // lookups can reuse the path-derived scope ids, and BEFORE membership
    // extraction: `subject.workspace_role` must reflect the entity's own
    // workspace (backfilled here), not just the path-named one.
    extract_entity_attributes(&state, &mut ctx, &pattern, req.uri().path(), resource_type).await;

    // Membership-sourced subject attributes (§Attributes: source "Membership",
    // "Memberships", "Team memberships"). The membership and team stores
    // are the canonical — and ONLY — source for these names: raw JWT claims
    // never define them (the agent-only claim allowlist above excludes the
    // names entirely). An empty/failed lookup leaves the attributes unset —
    // conditions on them then fail closed (a missing attribute never
    // matches), which is the safe direction.
    if let Some(user_id) = &auth.user_id {
        match state.workspace_memberships.list_by_user(user_id).await {
            Ok(memberships) if !memberships.is_empty() => {
                let workspace_ids: Vec<String> = memberships
                    .iter()
                    .map(|m| m.workspace_id.to_string())
                    .collect();
                ctx.set_list("subject.workspace_ids", workspace_ids);

                // Team memberships: the team store's per-user lookup
                // (list_for_user — teams in the workspace that count the
                // user as a member, via the TeamMembership store).
                let mut team_ids: Vec<String> = Vec::new();
                for membership in &memberships {
                    if let Ok(teams) = state
                        .teams
                        .list_for_user(&membership.workspace_id, user_id)
                        .await
                    {
                        for team in teams {
                            team_ids.push(team.id.to_string());
                        }
                    }
                }
                if !team_ids.is_empty() {
                    ctx.set_list("subject.team_ids", team_ids);
                }

                // `subject.workspace_role` (source "Membership") is the role
                // the caller holds in the workspace being addressed. When the
                // path names a workspace, use that membership's role; agents
                // and workspace-less requests have none.
                if let Some(ws_id) = ctx.get("resource.workspace_id").and_then(|v| match v {
                    AttrValue::Single(s) => Some(s.clone()),
                    _ => None,
                }) {
                    if let Some(m) = memberships
                        .iter()
                        .find(|m| m.workspace_id.to_string() == ws_id)
                    {
                        ctx.set("subject.workspace_role", m.role.as_str());
                    }
                }
            }
            Ok(_) => {} // no memberships — attributes stay unset
            Err(e) => {
                tracing::warn!(
                    user_id = %user_id,
                    err = %e,
                    "workspace membership lookup failed; membership subject attributes unset"
                );
            }
        }
    }

    // Environment attributes (§Attributes: env.ip, source "Request" — client
    // socket address). Forwarded headers are caller-controlled and are NOT
    // read (CWE-348; see check-forwarded-header-trust.sh).
    if let Some(peer) = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
        ctx.set("env.ip", peer.0.ip().to_string());
    }

    // Environment attributes (§Attributes): `env.budget_remaining` — tokens
    // remaining in the addressed workspace's daily budget. Computed from the
    // real budget system (BudgetUsageRepository + BudgetConfig); absent when
    // the path names no workspace or no budget is configured (conditions on
    // it then fail closed). `env.time` is injected by the evaluation engine.
    if let Some(ws_id) = ctx.get("resource.workspace_id").and_then(|v| match v {
        AttrValue::Single(s) => Some(s.clone()),
        _ => None,
    }) {
        let key = format!("workspace:{ws_id}");
        let config = state.budget_configs.get_config(&key).await.ok().flatten();
        let usage = state.budget_usages.get_usage(&key).await.ok().flatten();
        if let Some(remaining) = budget_tokens_remaining(config.as_ref(), usage.as_ref()) {
            ctx.set_number("env.budget_remaining", remaining);
        }
    }

    // Environment attributes (§Attributes): `env.main_health` — merge-queue
    // (forge) state for the addressed repo, from the real pause state read by
    // the merge processor and GET /repos/:id/status. Absent when the path
    // names no repo.
    if let Some(repo_id) = ctx.get("resource.repo_id").and_then(|v| match v {
        AttrValue::Single(s) => Some(s.clone()),
        _ => None,
    }) {
        ctx.set(
            "env.main_health",
            crate::merge_processor::queue_health(&state, &repo_id).await,
        );
    }

    // Load policies and evaluate.
    // Policies are loaded from the shared store on EVERY request — there is
    // no ABAC policy-result cache in gyre-server (only JWKS / graph /
    // dep-staleness caches exist, none of which cache policy decisions).
    // Trust-level transitions write workspace row + trust: policies through
    // this same store in one transaction, so any transition is visible to the
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
    use axum::{body::Body, routing::get, routing::post, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    // Shared test URIs. The ABAC registry check
    // (check-abac-route-registry.sh) greps this whole file for duplicate
    // `/api/v1/...` string literals to detect dead duplicate resolver
    // entries, so repeated test URIs are spelled once here and reused;
    // route patterns shared with the resolver registry use the module-level
    // consts above (SPECS_ROUTE, SPECS_PATH_ROUTE, REPOS_ID_ROUTE,
    // PERSONAS_ID_ROUTE).
    const SPEC_DOC_URI: &str = "/api/v1/specs/system%2Fdesign.md";
    const REPO_HEALTH_URI: &str = "/api/v1/repos/repo-health";
    const PERSONA_E_URI: &str = "/api/v1/personas/persona-e";
    const WS_TASKS_URI: &str = "/api/v1/workspaces/ws-1/tasks";
    const REPO_GATES_URI: &str = "/api/v1/repos/repo-1/gates";
    const REPO_WORKTREE_URI: &str = "/api/v1/repos/repo-1/worktrees/wt-42";
    const REPO_SPECS_ASSIST_URI: &str = "/api/v1/repos/repo-1/specs/assist";
    const MR_GATES_URI: &str = "/api/v1/merge-requests/mr-9/gates";
    const MR_DEPENDENCIES_URI: &str = "/api/v1/merge-requests/mr-9/dependencies";
    const AGENT_MESSAGES_URI: &str = "/api/v1/agents/ag-1/messages";

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

    /// Regression (task-171, HSI spec-amendment table): without the
    /// `developer-generate-access` Allow policy, `default-deny` blocks all
    /// LLM generation and prompt editing. The three LLM endpoints
    /// (`briefing/ask`, `explorer-views/generate`, `specs/assist`,
    /// `prompts/save`) use `action_override: "generate"`, which no other
    /// builtin Allow covers — a Developer-role JWT would be 403'd on every
    /// LLM call.
    ///
    /// Under the task-128 attribute model `subject.workspace_role` is
    /// sourced ONLY from the membership store — a JWT `workspace_role`
    /// claim never defines it (see
    /// `workspace_role_claim_cannot_spoof_membership_attribute`) — so the
    /// callers carry real memberships in the addressed workspace (the
    /// tokens still include a `workspace_role` claim, as a real IdP might;
    /// it is ignored). The repo-parented routes additionally need the repo
    /// row seeded so entity lookup can backfill `resource.workspace_id`
    /// from the repo's own workspace.
    #[tokio::test(flavor = "multi_thread")]
    async fn developer_jwt_can_generate_on_llm_endpoints() {
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        use axum::routing::post;
        use gyre_domain::{Repository, User, Workspace, WorkspaceMembership, WorkspaceRole};

        let state_base = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state_base))
                .expect("seed built-in policies")
        });
        init_resolver();

        // Addressed scope chain: workspace `some-ws` and repo `some-repo`
        // (parented by `some-ws`, so the repo-addressed LLM routes resolve
        // the caller's membership role through the repo's workspace).
        let ws_id = gyre_common::Id::new("some-ws");
        state_base
            .workspaces
            .create(&Workspace::new(
                ws_id.clone(),
                gyre_common::Id::new("tenant-1"),
                "some-ws".to_string(),
                "some-ws".to_string(),
                1000,
            ))
            .await
            .unwrap();
        state_base
            .repos
            .create(&Repository::new(
                gyre_common::Id::new("some-repo"),
                ws_id.clone(),
                "some-repo".to_string(),
                "/repos/some-repo".to_string(),
                0,
            ))
            .await
            .unwrap();

        // Pre-create the users the JWTs resolve to (external_id = sub)
        // with their workspace memberships: dev-llm-sub holds the
        // Developer workspace role, viewer-llm-sub the Viewer role (both
        // with the global Developer role, so the Viewer denial proves the
        // Allow is scoped to the workspace role, not global role or mere
        // membership presence).
        async fn seed_member(
            state: &AppState,
            user_id: &str,
            external_id: &str,
            membership_id: &str,
            workspace_role: WorkspaceRole,
        ) {
            let id = gyre_common::Id::new(user_id);
            let mut user = User::new(id.clone(), external_id, external_id, 0);
            user.roles = vec![gyre_domain::UserRole::Developer];
            state.users.create(&user).await.unwrap();
            let membership = WorkspaceMembership::new(
                gyre_common::Id::new(membership_id),
                id,
                gyre_common::Id::new("some-ws"),
                workspace_role,
                gyre_common::Id::new("inviter"),
                0,
            );
            state
                .workspace_memberships
                .create(&membership)
                .await
                .unwrap();
        }
        seed_member(
            &state_base,
            "dev-llm-user",
            "dev-llm-sub",
            "m-dev-llm",
            WorkspaceRole::Developer,
        )
        .await;
        seed_member(
            &state_base,
            "viewer-llm-user",
            "viewer-llm-sub",
            "m-viewer-llm",
            WorkspaceRole::Viewer,
        )
        .await;

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        // The LLM endpoints with `generate` action overrides.
        let app = Router::new()
            .route("/api/v1/workspaces/:id/briefing/ask", post(ok_handler))
            .route(
                "/api/v1/workspaces/:id/explorer-views/generate",
                post(ok_handler),
            )
            .route("/api/v1/repos/:id/specs/assist", post(ok_handler))
            .route("/api/v1/repos/:id/prompts/save", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state_base.clone(),
                abac_middleware,
            ))
            .with_state(state_base);

        let token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-llm-sub",
                "preferred_username": "developer",
                "realm_access": { "roles": ["developer"] },
                "workspace_role": "Developer"
            }),
            3600,
        );

        for path in [
            "/api/v1/workspaces/some-ws/briefing/ask",
            "/api/v1/workspaces/some-ws/explorer-views/generate",
            "/api/v1/repos/some-repo/specs/assist",
            "/api/v1/repos/some-repo/prompts/save",
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("Authorization", format!("Bearer {token}"))
                        .header("content-type", "application/json")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Developer must be allowed generate on {path}"
            );
        }

        // Viewer workspace role must still be denied — the Allow is
        // scoped to Developer/Admin workspace roles, not everyone.
        let viewer_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "viewer-llm-sub",
                "preferred_username": "viewer",
                "realm_access": { "roles": ["developer"] },
                "workspace_role": "Viewer"
            }),
            3600,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/some-ws/briefing/ask")
                    .header("Authorization", format!("Bearer {viewer_token}"))
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Viewer workspace role must be denied generate"
        );
    }

    // --- Membership-sourced subject attributes (§Attributes) -------------------

    /// Common setup: JWT-enabled state, a user pre-created via
    /// `find_or_create_user` (matching the JWT `sub`), a Viewer membership in
    /// `ws-1`, and membership in `team-platform` there. Returns (state, user).
    async fn setup_membership_state() -> (Arc<AppState>, gyre_common::Id) {
        use crate::auth::test_helpers::make_test_state_with_jwt;
        use gyre_domain::{Team, User, WorkspaceMembership, WorkspaceRole};

        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Pre-create the user the JWT will resolve to (external_id = sub).
        let user_id = gyre_common::Id::new("11111111-1111-1111-1111-111111111111");
        let mut user = User::new(user_id.clone(), "member-sub", "member-user", 0);
        user.roles = vec![gyre_domain::UserRole::Developer];
        state.users.create(&user).await.unwrap();

        // Viewer membership in ws-1.
        let membership = WorkspaceMembership::new(
            gyre_common::Id::new("m-1"),
            user_id.clone(),
            gyre_common::Id::new("ws-1"),
            WorkspaceRole::Viewer,
            gyre_common::Id::new("inviter"),
            0,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Team membership in ws-1 (team store + TeamMembership record).
        let team = Team::new(
            gyre_common::Id::new("team-platform"),
            gyre_common::Id::new("ws-1"),
            "Platform",
            "platform",
            user_id.clone(),
            0,
        );
        state.teams.create(&team).await.unwrap();
        state
            .teams
            .add_member(&gyre_domain::TeamMembership::new(
                gyre_common::Id::new("team-platform"),
                user_id.clone(),
                0,
            ))
            .await
            .unwrap();

        (state, user_id)
    }

    fn member_jwt() -> String {
        use crate::auth::test_helpers::sign_test_jwt;
        sign_test_jwt(
            &serde_json::json!({
                "sub": "member-sub",
                "preferred_username": "member-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        )
    }

    /// A Deny policy on `subject.workspace_role == Viewer` must fire for a
    /// user whose GLOBAL role is Developer but whose MEMBERSHIP role in the
    /// addressed workspace is Viewer. Fails when the middleware does not
    /// extract `subject.workspace_role` from the membership store: without
    /// it the Deny condition can't match and the global-role Developer Allow
    /// (priority 800) grants the request.
    #[tokio::test(flavor = "multi_thread")]
    async fn membership_workspace_role_denies_write_for_workspace_viewer() {
        use axum::routing::post;

        let (state, _user_id) = setup_membership_state().await;

        // Deny writes on workspace resources for workspace Viewers,
        // at a priority ABOVE the Developer global-role Allow (800).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-viewer-ws-deny"),
                name: "viewer-no-workspace-write".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.workspace_role".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("Viewer".to_string()),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/workspaces/:workspace_id/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(WS_TASKS_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "workspace Viewer membership must override global Developer allow"
        );
    }

    /// `subject.workspace_ids` and `subject.team_ids` must be extracted from
    /// the membership/team stores. A policy keyed on team membership must
    /// match. Fails when list-valued membership attributes are never
    /// populated.
    #[tokio::test(flavor = "multi_thread")]
    async fn membership_team_ids_populated_from_team_store() {
        use axum::routing::post;

        let (state, _user_id) = setup_membership_state().await;

        // Deny task writes for members of team-platform (via subject.team_ids).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-team-deny"),
                name: "team-platform-no-task-write".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.team_ids".to_string(),
                    operator: ConditionOp::Contains,
                    value: ConditionValue::String("team-platform".to_string()),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/workspaces/:workspace_id/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(WS_TASKS_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "subject.team_ids must be populated from the team store"
        );
    }

    // --- Review round 2: raw JWT claims must not define non-claim ---------
    // subject attributes (membership-sourced names, agent-claim names for
    // human subjects). A verified JWT's claim CONTENT is caller-chosen. ----

    /// Common setup for the claim-spoofing tests: JWT-enabled state with a
    /// ReadOnly user who holds NO workspace membership. On routes that do
    /// not address a workspace the membership block cannot extract (or
    /// overwrite) the membership-sourced attributes — the exact condition
    /// under which a raw claim used to define them.
    async fn setup_membershipless_state() -> (Arc<AppState>, gyre_common::Id) {
        use crate::auth::test_helpers::make_test_state_with_jwt;
        use gyre_domain::{User, UserRole};

        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        let user_id = gyre_common::Id::new("22222222-2222-2222-2222-222222222222");
        let mut user = User::new(user_id.clone(), "nomember-sub", "nomember-user", 0);
        user.roles = vec![UserRole::ReadOnly];
        state.users.create(&user).await.unwrap();

        (state, user_id)
    }

    /// A validly-signed HUMAN JWT whose claim set carries spoofed ABAC
    /// attributes: `workspace_role` (membership-sourced) plus `persona` and
    /// `attestation_level` (agent-claim-sourced). The signature verifies —
    /// the claim content is the attack surface (review round 2).
    fn spoof_claims_jwt() -> String {
        use crate::auth::test_helpers::sign_test_jwt;
        sign_test_jwt(
            &serde_json::json!({
                "sub": "nomember-sub",
                "preferred_username": "nomember-user",
                "realm_access": { "roles": ["readonly"] },
                "workspace_role": "Admin",
                "workspace_ids": ["ws-1"],
                "team_ids": ["team-platform"],
                "persona": "repo-orchestrator",
                "attestation_level": 3
            }),
            3600,
        )
    }

    /// Review round 2 (killing test): a membership-less user's raw JWT claim
    /// (`workspace_role: "Admin"`) must NOT define the membership-sourced
    /// `subject.workspace_role` on a repo-parented route, where the
    /// membership block has no addressed workspace to extract the role
    /// from. Before the claim allowlist + agent gate the claim defined the
    /// attribute and satisfied a workspace_role-keyed Allow end-to-end
    /// (200 OK); the correct behavior is that no policy matches and the
    /// builtin default deny stands.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_role_claim_cannot_spoof_membership_attribute() {
        let (state, _user_id) = setup_membershipless_state().await;

        // Tenant Allow keyed on the membership-sourced attribute, ABOVE the
        // builtin role allows (700-800) so only it can grant.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-spoof-ws-role-allow"),
                name: "spoof-ws-role-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 850,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.workspace_role".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("Admin".to_string()),
                }],
                actions: vec!["generate".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(REPOS_SPECS_ASSIST_ROUTE, post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = spoof_claims_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(REPO_SPECS_ASSIST_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a workspace_role claim must not satisfy a workspace_role-keyed Allow \
             for a membership-less caller on a repo-parented route"
        );
    }

    /// Review round 2 (same class, killing test): §Attributes sources
    /// `subject.persona` and `subject.attestation_level` from the "Agent
    /// OIDC claim" — a HUMAN token carrying them must not define them.
    /// Before the agent gate the claims merged (and normalized) for human
    /// subjects too, satisfying persona-keyed Allows end-to-end.
    #[tokio::test(flavor = "multi_thread")]
    async fn human_persona_claim_cannot_define_agent_attribute() {
        let (state, _user_id) = setup_membershipless_state().await;

        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-spoof-persona-allow"),
                name: "spoof-persona-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 850,
                effect: PolicyEffect::Allow,
                conditions: vec![
                    Condition {
                        attribute: "subject.persona".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("repo-orchestrator".to_string()),
                    },
                    Condition {
                        attribute: "subject.attestation_level".to_string(),
                        operator: ConditionOp::GreaterThan,
                        value: ConditionValue::Number(2),
                    },
                ],
                actions: vec!["generate".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(REPOS_SPECS_ASSIST_ROUTE, post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = spoof_claims_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(REPO_SPECS_ASSIST_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a human token's persona/attestation_level claims must not satisfy \
             an agent-claim-keyed Allow"
        );
    }

    /// Review round 2 (deny side): a real workspace Viewer whose token
    /// carries `workspace_role: "Admin"` must still trip a
    /// persona-management-style NotIn deny — the membership-extracted role
    /// (Viewer) is the value the deny evaluates, and claim merging may never
    /// overwrite it. This kills any reordering that merges claims AFTER
    /// membership extraction: the Admin claim would flip
    /// NotIn [Owner, Admin] to non-matching and let the builtin Developer
    /// Allow (priority 800) grant the write.
    #[tokio::test(flavor = "multi_thread")]
    async fn viewer_with_admin_claim_still_trips_notin_deny() {
        let (state, _user_id) = setup_membership_state().await;

        // Persona-management-style deny (§Policy Examples): workspace roles
        // outside {Owner, Admin} cannot write — above the Developer Allow.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-spoof-notin-deny"),
                name: "spoof-notin-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.workspace_role".to_string(),
                    operator: ConditionOp::NotIn,
                    value: ConditionValue::StringList(vec![
                        "Owner".to_string(),
                        "Admin".to_string(),
                    ]),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/workspaces/:workspace_id/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        // The member JWT (Viewer in ws-1) plus the spoofed Admin claim.
        let token = crate::auth::test_helpers::sign_test_jwt(
            &serde_json::json!({
                "sub": "member-sub",
                "preferred_username": "member-user",
                "realm_access": { "roles": ["developer"] },
                "workspace_role": "Admin"
            }),
            3600,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(WS_TASKS_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "the membership-extracted Viewer role must trip the NotIn deny; \
             the Admin claim must not overwrite it"
        );
    }

    /// Unit-level check of the path-attribute conventions on parent-scoped
    /// child routes (review round 1: the repos prefix lacked the
    /// `resource_type != "repo"` guard, so gate/worktree/policy resources
    /// carried the PARENT REPO id as resource.id, and
    /// /repos/:id/worktrees/:worktree_id never surfaced the worktree id).
    #[test]
    fn path_attributes_repos_parented_child_routes() {
        // Gate collection under a repo: `:id` names the repo (exposed as
        // resource.repo_id); no resource.id may be fabricated from it.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(&mut ctx, REPOS_GATES_ROUTE, REPO_GATES_URI, "gate");
        assert_eq!(attr_str(&ctx, "resource.repo_id"), Some("repo-1"));
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            None,
            "gate collection route must not expose the parent repo id as resource.id"
        );

        // Gate child route: `:gate_id` names the addressed gate.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            REPOS_GATES_ID_ROUTE,
            "/api/v1/repos/repo-1/gates/gate-9",
            "gate",
        );
        assert_eq!(attr_str(&ctx, "resource.repo_id"), Some("repo-1"));
        assert_eq!(attr_str(&ctx, "resource.id"), Some("gate-9"));

        // Worktree child route (the round-1 flagged route): the worktree id
        // surfaces, not the parent repo id.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            REPOS_WORKTREES_ID_ROUTE,
            REPO_WORKTREE_URI,
            "worktree",
        );
        assert_eq!(attr_str(&ctx, "resource.repo_id"), Some("repo-1"));
        assert_eq!(attr_str(&ctx, "resource.id"), Some("wt-42"));

        // Repo-typed repos routes keep `:id` as resource.id (regression
        // guard for the guard itself).
        let mut ctx = AttributeContext::default();
        extract_path_attributes(&mut ctx, REPOS_ID_ROUTE, "/api/v1/repos/repo-1", "repo");
        assert_eq!(attr_str(&ctx, "resource.repo_id"), Some("repo-1"));
        assert_eq!(attr_str(&ctx, "resource.id"), Some("repo-1"));

        // Workspace-parented suppression unchanged: the tasks collection
        // under a workspace exposes the workspace id, not a resource.id.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            "/api/v1/workspaces/:workspace_id/tasks",
            WS_TASKS_URI,
            "task",
        );
        assert_eq!(attr_str(&ctx, "resource.workspace_id"), Some("ws-1"));
        assert_eq!(attr_str(&ctx, "resource.id"), None);

        // Merge-request-parented dependency child route: the dependency id
        // surfaces via its `{resource_type}_id` param, not the parent MR id.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            "/api/v1/merge-requests/:id/dependencies/:dependency_id",
            "/api/v1/merge-requests/mr-1/dependencies/dep-3",
            "dependency",
        );
        assert_eq!(attr_str(&ctx, "resource.id"), Some("dep-3"));
    }

    /// Unit-level check of the path-attribute conventions on the
    /// merge-requests/agents parent-scoped collection routes (review
    /// round 4: the parent-id suppression guard covered only the
    /// workspaces/repos prefixes, so `/merge-requests/:id/gates`,
    /// `/merge-requests/:id/dependencies` and `/agents/:id/messages` — none
    /// of which carries a `{resource_type}_id` param — fabricated
    /// `resource.id` from the PARENT MR/agent id; any policy keyed on
    /// `resource.id` for gate/dependency/message types silently evaluated
    /// against the parent id).
    #[test]
    fn path_attributes_mr_and_agent_parented_child_routes() {
        // MR gate collection: `:id` names the parent MR; the addressed
        // entity is the gate collection, so no resource.id may be
        // fabricated from it (conditions on resource.id fail closed).
        let mut ctx = AttributeContext::default();
        extract_path_attributes(&mut ctx, MERGE_REQUESTS_GATES_ROUTE, MR_GATES_URI, "gate");
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            None,
            "gate collection route under an MR must not expose the parent MR id as resource.id"
        );

        // MR dependency collection: same convention.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            MERGE_REQUESTS_DEPENDENCIES_ROUTE,
            MR_DEPENDENCIES_URI,
            "dependency",
        );
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            None,
            "dependency collection route under an MR must not expose the parent MR id as resource.id"
        );

        // Agent message collection: `:id` names the parent agent.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            AGENTS_MESSAGES_ROUTE,
            AGENT_MESSAGES_URI,
            "message",
        );
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            None,
            "message collection route under an agent must not expose the parent agent id as resource.id"
        );

        // Controls — the parent-typed routes on the SAME prefixes keep
        // `:id` as resource.id (the generalized guard must not
        // over-suppress):
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            MERGE_REQUESTS_ID_ROUTE,
            "/api/v1/merge-requests/mr-9",
            "merge_request",
        );
        assert_eq!(attr_str(&ctx, "resource.id"), Some("mr-9"));

        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            AGENTS_ID_STATUS_ROUTE,
            "/api/v1/agents/ag-1/status",
            "agent",
        );
        assert_eq!(attr_str(&ctx, "resource.id"), Some("ag-1"));

        // The message child route surfaces the message id via its
        // `{resource_type}_id` param, not the parent agent id.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            AGENTS_MESSAGES_ACK_ROUTE,
            "/api/v1/agents/ag-1/messages/msg-7/ack",
            "message",
        );
        assert_eq!(attr_str(&ctx, "resource.id"), Some("msg-7"));
    }

    /// Registry-wide invariant (review round 4): no registered route may
    /// fabricate `resource.id` from a PARENT entity id. Walks EVERY
    /// registered non-exempt route, replays path extraction against a
    /// synthetic concrete path (`:id` → a recognizable sentinel), and
    /// asserts:
    /// - on parent-scoped child routes (`:id` followed by more segments,
    ///   registered resource type ≠ the prefix collection's entity type),
    ///   `resource.id` never carries the `:id` sentinel;
    /// - on entity routes (`:id` names the route's own resource type and
    ///   the route has no `{resource_type}_id`/`:path` param), `resource.id`
    ///   DOES carry the `:id` sentinel — the guard must not over-suppress.
    ///
    /// The classification uses the same helpers as the guard, so this test
    /// does not re-derive the convention — it pins the RESULT on the live
    /// registry: a future route registered under a new parent-scoped prefix
    /// without a `{resource_type}_id` param fails the first assertion (the
    /// round-4 defect class, caught for every prefix at once), and a guard
    /// that suppresses too broadly fails the second.
    #[test]
    fn registry_routes_never_leak_parent_id_into_resource_id() {
        init_resolver();
        let resolver = RESOURCE_RESOLVER.get().unwrap();
        let parent_value = "PARENT-ID";
        for mapping in &resolver.routes {
            if mapping.exempt {
                continue;
            }
            let pattern = mapping.pattern;
            let resource_type = mapping.resource_type;

            // Synthetic concrete path: each `:param` binds a distinct
            // recognizable value (`:id` → the parent sentinel, `:path` → a
            // plainly decodable slug).
            let path = pattern
                .split('/')
                .map(|seg| match seg.strip_prefix(':') {
                    Some("id") => parent_value.to_string(),
                    Some("path") => "doc.md".to_string(),
                    Some(name) => format!("VAL-{name}"),
                    None => seg.to_string(),
                })
                .collect::<Vec<_>>()
                .join("/");

            let mut ctx = AttributeContext::default();
            extract_path_attributes(&mut ctx, pattern, &path, resource_type);

            let child_param = format!(":{resource_type}_id");
            let names_child = pattern.split('/').any(|s| s == child_param);
            let has_id = pattern.split('/').any(|s| s == ":id");
            let parent_scoped = has_child_segments_after_id(pattern)
                && prefix_entity_type(pattern).is_some_and(|entity| entity != resource_type);

            if parent_scoped {
                assert_ne!(
                    attr_str(&ctx, "resource.id"),
                    Some(parent_value),
                    "{pattern}: parent-scoped child route (resource type {resource_type}) \
                     must not leak the parent `:id` into resource.id"
                );
            } else if has_id && !names_child && !pattern.contains("/:path") {
                assert_eq!(
                    attr_str(&ctx, "resource.id"),
                    Some(parent_value),
                    "{pattern}: `:id` names the {resource_type} entity — resource.id must carry it"
                );
            }
        }
    }

    /// Unit-level check of the `:path` convention (review round 3: spec
    /// routes address the resource by a URL-encoded registry path, and
    /// `resource.id` was never populated — no `:id` or `{resource_type}_id`
    /// param exists on those routes, so no policy could condition on the
    /// addressed spec's identity).
    #[test]
    fn path_attributes_path_param_names_addressed_spec() {
        // Spec document route: the percent-DECODED registry path is the
        // resource identity (§Attributes example: `system/identity-security.md`).
        let mut ctx = AttributeContext::default();
        extract_path_attributes(&mut ctx, SPECS_PATH_ROUTE, SPEC_DOC_URI, "spec");
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            Some("system/design.md"),
            "resource.id on a :path-addressed spec route must be the decoded registry path"
        );

        // Spec child route: still the decoded spec path, not the literal
        // `history` segment or the raw encoded param. (Pattern derived from
        // SPECS_PATH_ROUTE so no new `/api/v1/...` literal is introduced —
        // the registry check freezes the duplicate-literal count.)
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            &format!("{SPECS_PATH_ROUTE}/history"),
            "/api/v1/specs/system%2Fdesign.md/history",
            "spec",
        );
        assert_eq!(attr_str(&ctx, "resource.id"), Some("system/design.md"));

        // Repo-parented spec child route: `:id` names the parent repo
        // (resource.repo_id), `:path` names the addressed spec. (Pattern
        // derived from REPOS_ID_ROUTE for the same duplicate-literal reason.)
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            &format!("{REPOS_ID_ROUTE}/specs/:path/assertions"),
            "/api/v1/repos/repo-1/specs/system%2Fdesign.md/assertions",
            "spec",
        );
        assert_eq!(attr_str(&ctx, "resource.repo_id"), Some("repo-1"));
        assert_eq!(attr_str(&ctx, "resource.id"), Some("system/design.md"));

        // Invalid percent-encoding cannot name an entity: `resource.id`
        // stays unset so conditions on it fail closed.
        let mut ctx = AttributeContext::default();
        extract_path_attributes(
            &mut ctx,
            SPECS_PATH_ROUTE,
            "/api/v1/specs/bad%ZZ.md",
            "spec",
        );
        assert_eq!(
            attr_str(&ctx, "resource.id"),
            None,
            "invalid percent-encoding must leave resource.id unset (fail closed)"
        );
    }

    /// Review round 3 (doc/code divergence): `normalize_agent_jwt_attributes`
    /// documents "never overwrites an attribute already present in the
    /// context" — the exact invariant the round-2 anti-spoofing fix relies
    /// on — but its setters were unconditional. A future reordering (calling
    /// the normalizer before extractor-resolved attributes exist, or with a
    /// different claims source) would silently violate the stated guarantee
    /// with no test catching it. This test pins the invariant directly.
    #[test]
    fn normalize_agent_jwt_attributes_never_overwrites_existing_attributes() {
        let mut ctx = AttributeContext::default();
        // Pre-set values the normalizer must not clobber (e.g. set by an
        // earlier extraction pass from a different source).
        ctx.set("subject.stack_hash", "sha256:already-set");
        ctx.set("subject.repo_scope", "repo-already");
        ctx.set_number("subject.attestation_level", 1);

        let claims = serde_json::json!({
            "wl_stack_hash": "sha256:claim-value",
            "repo_id": "repo-claim",
            "attestation_level": 3
        });
        normalize_agent_jwt_attributes(&mut ctx, &claims);

        assert_eq!(
            ctx.get("subject.stack_hash"),
            Some(&AttrValue::Single("sha256:already-set".to_string()))
        );
        assert_eq!(
            ctx.get("subject.repo_scope"),
            Some(&AttrValue::Single("repo-already".to_string()))
        );
        assert_eq!(
            ctx.get("subject.attestation_level"),
            Some(&AttrValue::Number(1))
        );
    }

    /// Review round 1 (code finding), live-middleware reproduction: on the
    /// gate collection route under a repo, `resource.id` must NOT carry the
    /// parent repo id. A Deny keyed on `resource.id == <repo id>` must not
    /// fire; the same Deny keyed on `resource.repo_id` (which DOES name the
    /// parent repo) must fire — proving the policy machinery is live on
    /// this very route, so the non-deny is not vacuous.
    #[tokio::test(flavor = "multi_thread")]
    async fn repos_parented_collection_route_does_not_leak_parent_repo_id() {
        use axum::routing::get;

        let (state, _user_id) = setup_membership_state().await;

        // Deny gate reads when resource.id equals the parent repo id. With
        // the round-1 defect (resource.id = repo id) this fires → 403;
        // correct behavior leaves resource.id unset, the condition fails,
        // and the Developer role Allow (priority 800) grants the read.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-gate-resource-id-deny"),
                name: "gate-resource-id-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("repo-1".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["gate".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(REPOS_GATES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(REPO_GATES_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "resource.id must not carry the parent repo id on the gate collection route"
        );

        // Control on a fresh state: the same Deny keyed on resource.repo_id
        // (present — the `:id` names the parent repo) must fire.
        let (state2, _user_id2) = setup_membership_state().await;
        state2
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-gate-repo-id-deny"),
                name: "gate-repo-id-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.repo_id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("repo-1".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["gate".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        let app2 = Router::new()
            .route(REPOS_GATES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state2.clone(),
                abac_middleware,
            ))
            .with_state(state2);

        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(REPO_GATES_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp2.status(),
            StatusCode::FORBIDDEN,
            "control: resource.repo_id names the parent repo — the Deny must fire"
        );
    }

    /// Review round 4 (code finding), live-middleware reproduction: on the
    /// gate and dependency collection routes under a merge request,
    /// `resource.id` must NOT carry the parent MR id. A Deny keyed on
    /// `resource.id == <mr id>` must not fire; a control Deny keyed on a
    /// subject attribute (extracted for every caller) must fire on the very
    /// same routes — proving the policy machinery is live there, so the
    /// non-deny is not vacuous. Mirrors the repos-prefix coverage
    /// (`repos_parented_collection_route_does_not_leak_parent_repo_id`).
    #[tokio::test(flavor = "multi_thread")]
    async fn mr_parented_collection_routes_do_not_leak_parent_mr_id() {
        use axum::routing::get;

        let (state, _user_id) = setup_membership_state().await;

        // Deny gate/dependency reads when resource.id equals the parent MR
        // id. With the round-4 defect (resource.id = MR id) these fire →
        // 403; correct behavior leaves resource.id unset, the conditions
        // fail, and the Developer role Allow (priority 800) grants the read.
        for (policy_id, name, resource_type) in [
            (
                "test-mr-gate-resource-id-deny",
                "mr-gate-resource-id-deny",
                "gate",
            ),
            (
                "test-mr-dep-resource-id-deny",
                "mr-dep-resource-id-deny",
                "dependency",
            ),
        ] {
            state
                .policies
                .create(&Policy {
                    id: gyre_common::Id::new(policy_id),
                    name: name.to_string(),
                    description: String::new(),
                    scope: PolicyScope::Tenant,
                    scope_id: None,
                    priority: 810,
                    effect: PolicyEffect::Deny,
                    conditions: vec![Condition {
                        attribute: "resource.id".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("mr-9".to_string()),
                    }],
                    actions: vec!["read".to_string()],
                    resource_types: vec![resource_type.to_string()],
                    enabled: true,
                    built_in: false,
                    immutable: false,
                    created_by: Id::new("system"),
                    created_at: 0,
                    updated_at: 0,
                })
                .await
                .unwrap();
        }

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(MERGE_REQUESTS_GATES_ROUTE, get(ok_handler))
            .route(MERGE_REQUESTS_DEPENDENCIES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        for uri in [MR_GATES_URI, MR_DEPENDENCIES_URI] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(uri)
                        .header("Authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "resource.id must not carry the parent MR id on {uri}"
            );
        }

        // Control on a fresh state: the same Denies keyed on
        // subject.global_role — extracted for every caller — must fire on
        // these very routes, proving evaluation is live.
        let (state2, _user_id2) = setup_membership_state().await;
        for (policy_id, name, resource_type) in [
            ("test-mr-gate-subject-deny", "mr-gate-subject-deny", "gate"),
            (
                "test-mr-dep-subject-deny",
                "mr-dep-subject-deny",
                "dependency",
            ),
        ] {
            state2
                .policies
                .create(&Policy {
                    id: gyre_common::Id::new(policy_id),
                    name: name.to_string(),
                    description: String::new(),
                    scope: PolicyScope::Tenant,
                    scope_id: None,
                    priority: 810,
                    effect: PolicyEffect::Deny,
                    conditions: vec![Condition {
                        attribute: "subject.global_role".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("Developer".to_string()),
                    }],
                    actions: vec!["read".to_string()],
                    resource_types: vec![resource_type.to_string()],
                    enabled: true,
                    built_in: false,
                    immutable: false,
                    created_by: Id::new("system"),
                    created_at: 0,
                    updated_at: 0,
                })
                .await
                .unwrap();
        }

        let app2 = Router::new()
            .route(MERGE_REQUESTS_GATES_ROUTE, get(ok_handler))
            .route(MERGE_REQUESTS_DEPENDENCIES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state2.clone(),
                abac_middleware,
            ))
            .with_state(state2);

        for uri in [MR_GATES_URI, MR_DEPENDENCIES_URI] {
            let resp = app2
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(uri)
                        .header("Authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "control: subject-keyed Deny must fire on {uri} — evaluation is live"
            );
        }
    }

    /// Review round 4 (code finding), live-middleware reproduction: on the
    /// message collection route under an agent, `resource.id` must NOT
    /// carry the parent agent id. Same structure as the MR coverage above.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_parented_message_route_does_not_leak_parent_agent_id() {
        use axum::routing::get;

        let (state, _user_id) = setup_membership_state().await;

        // Deny message reads when resource.id equals the parent agent id.
        // With the round-4 defect (resource.id = agent id) this fires →
        // 403; correct behavior leaves resource.id unset.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-message-resource-id-deny"),
                name: "message-resource-id-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("ag-1".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["message".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(AGENTS_MESSAGES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(AGENT_MESSAGES_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "resource.id must not carry the parent agent id on the message collection route"
        );

        // Control on a fresh state: the same Deny keyed on
        // subject.global_role must fire on this very route.
        let (state2, _user_id2) = setup_membership_state().await;
        state2
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-message-subject-deny"),
                name: "message-subject-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.global_role".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("Developer".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["message".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        let app2 = Router::new()
            .route(AGENTS_MESSAGES_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state2.clone(),
                abac_middleware,
            ))
            .with_state(state2);

        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(AGENT_MESSAGES_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp2.status(),
            StatusCode::FORBIDDEN,
            "control: subject-keyed Deny must fire on the message collection route"
        );
    }

    /// Review round 1 (code finding), live-middleware reproduction: the
    /// worktree child route must surface the worktree id as resource.id.
    /// DELETE maps to action "delete", which no built-in role policy
    /// allows, so only the condition-bearing Allow can grant — it keys on
    /// resource.id == the worktree id. With the round-1 defect resource.id
    /// carried the parent repo id and the Allow could not match.
    #[tokio::test(flavor = "multi_thread")]
    async fn repos_parented_child_route_surfaces_worktree_id_as_resource_id() {
        use axum::routing::delete;

        let (state, _user_id) = setup_membership_state().await;

        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-worktree-id-allow"),
                name: "worktree-id-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "resource.id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("wt-42".to_string()),
                }],
                actions: vec!["delete".to_string()],
                resource_types: vec!["worktree".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(REPOS_WORKTREES_ID_ROUTE, delete(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(REPO_WORKTREE_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "the worktree id must surface as resource.id on /repos/:id/worktrees/:worktree_id"
        );

        // Control on a fresh state: an Allow keyed on the leaked parent
        // value ("repo-1") must NOT grant — resource.id is the worktree id.
        let (state2, _user_id2) = setup_membership_state().await;
        state2
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-worktree-parent-id-allow"),
                name: "worktree-parent-id-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "resource.id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("repo-1".to_string()),
                }],
                actions: vec!["delete".to_string()],
                resource_types: vec!["worktree".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        let app2 = Router::new()
            .route(REPOS_WORKTREES_ID_ROUTE, delete(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state2.clone(),
                abac_middleware,
            ))
            .with_state(state2);

        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(REPO_WORKTREE_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp2.status(),
            StatusCode::FORBIDDEN,
            "control: the parent repo id must not surface as resource.id"
        );
    }

    /// `env.ip` must be extracted from the request's ConnectInfo extension
    /// (socket peer address). Fails when env.ip is never populated.
    #[tokio::test(flavor = "multi_thread")]
    async fn env_ip_extracted_from_connect_info() {
        use axum::routing::get;

        let (state, _user_id) = setup_membership_state().await;

        // Allow reads only for requests from 10.0.0.5 — the policy only
        // matches when env.ip is extracted from ConnectInfo. Default deny
        // (priority 1) makes a missing env.ip deny the request.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-env-ip-allow"),
                name: "env-ip-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 950,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "env.ip".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("10.0.0.5".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/workspaces/:workspace_id/tasks", get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let token = member_jwt();
        let peer: std::net::SocketAddr = "10.0.0.5:4242".parse().unwrap();
        let mut req = Request::builder()
            .uri(WS_TASKS_URI)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(peer));

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "env.ip must be extracted from ConnectInfo and allow the pinned peer"
        );
    }

    /// Agent-JWT claims must reach live policy evaluation (§Attributes:
    /// persona, stack_hash, attestation_level, repo_scope — source "Agent
    /// OIDC claim"). Mints a real agent JWT via the signing key, registers it
    /// in agent_tokens (the auth path the middleware exercises), and asserts
    /// a Deny keyed on the claim's normalized attribute fires.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_jwt_claims_reach_live_evaluation() {
        use axum::routing::get;

        let state = crate::mem::test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Mint a worker JWT scoped to repo-7 with a stack hash claim.
        let token = state
            .agent_signing_key
            .mint_worker_scoped(
                "agent-claims-1",
                "task-1",
                "spawner",
                &state.base_url,
                3600,
                // §1: hierarchy claims (tenant, workspace, repo) so the
                // fine-grained `repo:repo-7:write` scope list is minted from
                // real context.
                "default",
                "ws-claims",
                "repo-7",
                "repo-7",
                None,
                None,
                None,
            )
            .unwrap();
        // Register it so the auth extractor resolves the agent identity.
        state
            .kv_store
            .kv_set("agent_tokens", "agent-claims-1", token.clone())
            .await
            .unwrap();

        // Deny read on spec resources for agents whose repo scope is repo-7.
        // subject.repo_scope only exists when the middleware normalizes the
        // JWT's repo_id claim — without it the condition fails closed and the
        // Agent-role Allow (priority 700) grants the request.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-agent-repo-scope-deny"),
                name: "agent-repo-7-no-spec-read".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![
                    Condition {
                        attribute: "subject.type".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("agent".to_string()),
                    },
                    Condition {
                        attribute: "subject.repo_scope".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("repo-7".to_string()),
                    },
                ],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPECS_ROUTE)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "subject.repo_scope from the agent JWT must reach live evaluation and deny"
        );
    }

    /// The `wl_stack_hash` agent-JWT claim must be normalized onto the
    /// spec-named `subject.stack_hash` attribute (§Attributes).
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_jwt_wl_stack_hash_normalized() {
        use axum::routing::get;

        let state = crate::mem::test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Mint with workload claims directly (§1: hierarchy context included
        // so the scope list is minted from real spawn-shaped context).
        let token = state
            .agent_signing_key
            .mint_with_workload(
                "agent-claims-2",
                "task-2",
                "spawner",
                &state.base_url,
                3600,
                "default",
                "ws-claims",
                "repo-7",
                "repo-7",
                None,
                None,
                Some(4242),
                Some("host-1".to_string()),
                Some("local".to_string()),
                Some("sha256:stack-abc".to_string()),
                None,
                None,
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-claims-2", token.clone())
            .await
            .unwrap();

        // Allow spec reads only for agents with this exact stack hash.
        //
        // The blanket agent-role Allow (builtin agent-scoped-access, priority
        // 700) would grant the 200 even when subject.stack_hash is absent —
        // this test would then be self-confirming. A Deny at 705 (between
        // the builtin Allow at 700 and this Allow at 720) closes that path:
        // the ONLY route to 200 is the stack-hash-keyed Allow above it.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-stack-hash-allow"),
                name: "stack-hash-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 720,
                effect: PolicyEffect::Allow,
                conditions: vec![Condition {
                    attribute: "subject.stack_hash".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("sha256:stack-abc".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        // Discriminating Deny at 705: blocks the builtin agent-role Allow at
        // 700 so a 200 requires the stack-hash condition to match.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-stack-hash-deny"),
                name: "stack-hash-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 705,
                effect: PolicyEffect::Deny,
                conditions: vec![],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPECS_ROUTE)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "subject.stack_hash (normalized from wl_stack_hash) must allow the pinned stack"
        );
    }

    /// §Attributes: `subject.persona` and `subject.attestation_level` are
    /// sourced from the "Agent OIDC claim" — the persona claim merges raw and
    /// the attestation_level claim is normalized to a number. Both must reach
    /// live evaluation from a real minted JWT.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_jwt_persona_and_attestation_level_reach_live_evaluation() {
        let state = crate::mem::test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Two agents: one with a persona + full attestation, one with neither.
        let strong = state
            .agent_signing_key
            .mint_worker_scoped(
                "agent-persona-1",
                "task-1",
                "spawner",
                &state.base_url,
                3600,
                "default",
                "ws-persona",
                "repo-1",
                "repo-1",
                Some("repo-orchestrator"),
                Some(3),
                None,
            )
            .unwrap();
        let weak = state
            .agent_signing_key
            .mint_worker_scoped(
                "agent-persona-2",
                "task-1",
                "spawner",
                &state.base_url,
                3600,
                "default",
                "ws-persona",
                "repo-1",
                "repo-1",
                None,
                Some(1),
                None,
            )
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-persona-1", strong.clone())
            .await
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-persona-2", weak.clone())
            .await
            .unwrap();

        // Allow spec writes only for repo-orchestrator personas with
        // attestation level >= 3. The spec's 8 operators have no >=, so the
        // bound is expressed as GreaterThan 2 (levels are the integers 1-3;
        // the spec's own gate-approved-persona example expresses the same
        // bound as a Deny on less_than 3). Both conditions fail closed when
        // the claims are missing, so the un-attested agent falls through to
        // default-deny (priority 1).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-persona-attestation-allow"),
                name: "persona-attestation-allow".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 730,
                effect: PolicyEffect::Allow,
                actions: vec!["write".to_string()],
                resource_types: vec!["spec".to_string()],
                conditions: vec![
                    Condition {
                        attribute: "subject.persona".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("repo-orchestrator".to_string()),
                    },
                    Condition {
                        attribute: "subject.attestation_level".to_string(),
                        operator: ConditionOp::GreaterThan,
                        value: ConditionValue::Number(2),
                    },
                ],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        // Discriminating Deny at 715: without it the builtin agent-role
        // Allow (agent-scoped-access, priority 700) grants the weak agent a
        // 200 even when the persona/attestation conditions fail — the test
        // would be self-confirming. With the Deny between the builtin Allow
        // (700) and this test Allow (730), the ONLY path to 200 is the
        // persona+attestation-keyed Allow above it.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-persona-attestation-deny"),
                name: "persona-attestation-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 715,
                effect: PolicyEffect::Deny,
                conditions: vec![],
                actions: vec!["write".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_ROUTE, axum::routing::post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let ok = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(SPECS_ROUTE)
                    .header("Authorization", format!("Bearer {strong}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ok.status(),
            StatusCode::OK,
            "persona + attestation_level >= 3 claims must reach live evaluation and allow"
        );

        let denied = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(SPECS_ROUTE)
                    .header("Authorization", format!("Bearer {weak}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            denied.status(),
            StatusCode::FORBIDDEN,
            "agent without the persona claim must fail closed (persona condition unmet)"
        );
    }

    /// `subject.tenant_id` is set from the auth context for every auth path,
    /// including agent tokens (which carry no JWT claims object).
    #[tokio::test(flavor = "multi_thread")]
    async fn subject_tenant_id_set_for_agent_token_auth() {
        use axum::routing::get;

        let state = crate::mem::test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        // Raw agent token (no JWT): tenant_id can only come from the
        // auth context line in the middleware.
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agent-tenant-1",
                "raw-uuid-token".to_string(),
            )
            .await
            .unwrap();

        // Deny read on specs for subjects in tenant "default" (the tenant
        // mem::test_state scopes everything to).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-tenant-deny"),
                name: "tenant-default-no-spec-read".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 820,
                effect: PolicyEffect::Deny,
                conditions: vec![
                    Condition {
                        attribute: "subject.type".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("agent".to_string()),
                    },
                    Condition {
                        attribute: "subject.tenant_id".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("default".to_string()),
                    },
                ],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPECS_ROUTE)
                    .header("Authorization", "Bearer raw-uuid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "subject.tenant_id from the auth context must reach live evaluation for agent tokens"
        );
    }

    /// `env.budget_remaining` must be extracted from the real budget system
    /// (BudgetConfig + BudgetUsage) for the addressed workspace. A policy
    /// keyed on it must match; with no budget configured the attribute must
    /// stay absent (fail closed).
    #[tokio::test(flavor = "multi_thread")]
    async fn env_budget_remaining_from_real_budget_system() {
        use axum::routing::post;
        use gyre_domain::BudgetConfig;

        let (state, _user_id) = setup_membership_state().await;

        // Configure a real budget for ws-1: 1000 tokens/day, 400 used.
        state
            .budget_configs
            .set_config(
                &crate::api::budget::workspace_key("ws-1"),
                &BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        state
            .budget_usages
            .set_usage(
                &crate::api::budget::workspace_key("ws-1"),
                &gyre_domain::BudgetUsage {
                    entity_type: "workspace".to_string(),
                    entity_id: gyre_common::Id::new("ws-1"),
                    tokens_used_today: 400,
                    cost_today: 0.0,
                    compute_minutes: 0,
                    active_agents: 0,
                    period_start: 0,
                },
            )
            .await
            .unwrap();

        // Deny task writes when fewer than 500 tokens remain (1000 - 400 = 600
        // → no match, Developer allow at 800 grants). Below the threshold the
        // Deny (810) outranks the Developer allow and must flip to 403.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-budget-deny"),
                name: "budget-remaining-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "env.budget_remaining".to_string(),
                    operator: ConditionOp::LessThan,
                    value: ConditionValue::Number(500),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/workspaces/:workspace_id/tasks", post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let token = member_jwt();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(WS_TASKS_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "env.budget_remaining (1000-400=600, not < 500) must not trip the deny"
        );

        // Burn the budget down to 950 used → 50 remaining < 500 → deny.
        state
            .budget_usages
            .set_usage(
                &crate::api::budget::workspace_key("ws-1"),
                &gyre_domain::BudgetUsage {
                    entity_type: "workspace".to_string(),
                    entity_id: gyre_common::Id::new("ws-1"),
                    tokens_used_today: 950,
                    cost_today: 0.0,
                    compute_minutes: 0,
                    active_agents: 0,
                    period_start: 0,
                },
            )
            .await
            .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(WS_TASKS_URI)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "env.budget_remaining (50 < 500) must flip the decision to deny"
        );
    }

    /// `env.main_health` must be extracted from the real merge-queue pause
    /// state for the addressed repo: `"red"` when the queue is paused,
    /// `"green"` otherwise (§Attributes, source "Forge state").
    #[tokio::test(flavor = "multi_thread")]
    async fn env_main_health_from_real_forge_state() {
        use axum::routing::post;

        let (state, _user_id) = setup_membership_state().await;

        // Deny repo writes while the merge queue is paused (env.main_health
        // == "red"). At 810 it outranks the Developer write allow (800);
        // while the queue runs (green) the Deny does not match and the
        // Developer allow grants the write.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-main-health-deny"),
                name: "main-health-red-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "env.main_health".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("red".to_string()),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["repo".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(REPOS_ID_ROUTE, post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let token = member_jwt();
        let req = |method: &str, uri: &str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };

        // No pause state recorded → queue green → Developer allow grants.
        let resp = app
            .clone()
            .oneshot(req("POST", REPO_HEALTH_URI))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "env.main_health must read 'green' for an unpaused merge queue (deny not tripped)"
        );

        // Pause the queue for this repo via the real pause path → red → deny.
        let repo = gyre_domain::Repository::new(
            gyre_common::Id::new("repo-health"),
            gyre_common::Id::new("ws-1"),
            "repo-health".to_string(),
            "/tmp/repo-health".to_string(),
            0,
        );
        crate::merge_processor::pause_merge_queue(&state, &repo, "test pause").await;

        let resp = app.oneshot(req("POST", REPO_HEALTH_URI)).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "env.main_health must read 'red' for a paused merge queue and deny"
        );
    }

    // --- §Attributes: entity-lookup / entity-field resource attributes ---

    /// Seed the scope chain tenant `t-1` → workspace `ws-e` → repo `repo-e`,
    /// plus a pending-approval spec `system/design.md` owned by `user:alice`
    /// scoped to the repo. Returns the seeded state.
    async fn setup_entity_state() -> Arc<AppState> {
        use crate::auth::test_helpers::make_test_state_with_jwt;
        use gyre_domain::{
            ApprovalStatus, Repository as DomainRepo, SpecLedgerEntry, Tenant, Workspace,
        };

        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state)).expect("seed");
        });

        state
            .tenants
            .create(&Tenant::new(Id::new("t-1"), "Acme", "acme", 0))
            .await
            .unwrap();
        state
            .workspaces
            .create(&Workspace::new(
                Id::new("ws-e"),
                Id::new("t-1"),
                "Eng",
                "eng",
                0,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&DomainRepo::new(
                Id::new("repo-e"),
                Id::new("ws-e"),
                "app",
                "/tmp/repo-e",
                0,
            ))
            .await
            .unwrap();

        state
            .spec_ledger
            .save(&SpecLedgerEntry {
                path: "system/design.md".to_string(),
                title: "Design".to_string(),
                owner: "user:alice".to_string(),
                kind: None,
                current_sha: "a".repeat(40),
                approval_mode: "human_only".to_string(),
                approval_status: ApprovalStatus::Pending,
                linked_tasks: vec![],
                linked_mrs: vec![],
                drift_status: "unknown".to_string(),
                created_at: 0,
                updated_at: 0,
                repo_id: Some("repo-e".to_string()),
                workspace_id: Some("ws-e".to_string()),
            })
            .await
            .unwrap();

        state
    }

    /// `resource.owner` and `resource.approval_status` must come from the
    /// spec ledger entry (§Attributes, source "Entity field"), and
    /// `resource.tenant_id` from the entity's scope chain. A policy keyed on
    /// the owner must match; a policy keyed on a different owner must not.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_entity_attributes_reach_live_evaluation() {
        let state = setup_entity_state().await;

        // Deny spec reads unless the resource owner is user:alice (810
        // outranks the Developer read allow at 800).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-owner-deny"),
                name: "owner-not-alice-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.owner".to_string(),
                    operator: ConditionOp::NotEquals,
                    value: ConditionValue::String("user:alice".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_PATH_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        // The seeded spec's owner IS user:alice → the NotEquals deny does not
        // match → the Developer read allow (800) grants.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(SPEC_DOC_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "resource.owner == user:alice must reach live evaluation (NotEquals deny not tripped)"
        );

        // Flip the ledger entry's owner to user:bob → NotEquals now matches
        // → deny.
        let mut entry = state
            .spec_ledger
            .find_by_path("system/design.md")
            .await
            .unwrap()
            .expect("seeded entry");
        entry.owner = "user:bob".to_string();
        state.spec_ledger.save(&entry).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPEC_DOC_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "resource.owner == user:bob must trip the NotEquals deny"
        );
    }

    /// `resource.id` on `:path`-addressed spec routes must be the
    /// percent-DECODED registry path and reach live evaluation (review
    /// round 3: no `:id`/`{resource_type}_id` param exists on those routes,
    /// so `resource.id` was never populated and no policy could condition
    /// on the addressed spec's identity — §Attributes defines resource.id
    /// with the spec-path example `system/identity-security.md`).
    ///
    /// Discriminating form: a Deny at priority 810 (above the builtin
    /// Developer read allow at 800) keyed on `resource.id Equals
    /// "system/design.md"`. Without the attribute the condition fails
    /// closed, the builtin allow grants, and the request returns 200 — the
    /// killing direction. The raw (still-encoded) param value would NOT
    /// match the decoded constant, so the deny firing also proves the
    /// percent-decoding. A second Deny keyed on a DIFFERENT spec path must
    /// not fire, proving the attribute is the addressed spec's path, not a
    /// constant.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_resource_id_from_decoded_path_reaches_live_evaluation() {
        let state = setup_entity_state().await;

        let deny_policy = |path: &str| Policy {
            id: gyre_common::Id::new("test-spec-resource-id-deny"),
            name: "spec-path-deny".to_string(),
            description: String::new(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 810,
            effect: PolicyEffect::Deny,
            conditions: vec![Condition {
                attribute: "resource.id".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String(path.to_string()),
            }],
            actions: vec!["read".to_string()],
            resource_types: vec!["spec".to_string()],
            enabled: true,
            built_in: false,
            immutable: false,
            created_by: Id::new("system"),
            created_at: 0,
            updated_at: 0,
        };

        // Deny reads of exactly system/design.md (the seeded spec's path).
        state
            .policies
            .create(&deny_policy("system/design.md"))
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_PATH_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        // GET /api/v1/specs/system%2Fdesign.md: the decoded resource.id
        // matches the deny's constant → 403. (Without resource.id populated
        // — the round-3 defect — the builtin Developer read allow at 800
        // grants and this returns 200.)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(SPEC_DOC_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "resource.id (decoded spec path) must reach live evaluation and deny"
        );

        // Retarget the deny to a different spec path: the addressed spec's
        // resource.id must not match → the builtin Developer read allow
        // grants → 200. Proves the attribute carries the addressed spec's
        // path, not any constant.
        state
            .policies
            .delete("test-spec-resource-id-deny")
            .await
            .unwrap();
        state
            .policies
            .create(&deny_policy("system/other.md"))
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPEC_DOC_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a deny keyed on a different spec path must not fire"
        );
    }

    /// `resource.approval_status` must flow from the ledger entry into
    /// evaluation: a deny keyed on `pending` must fire only while the spec
    /// is pending.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_approval_status_reaches_live_evaluation() {
        let state = setup_entity_state().await;

        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-pending-deny"),
                name: "pending-spec-no-write".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.approval_status".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("pending".to_string()),
                }],
                actions: vec!["write".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_PATH_ROUTE, post(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let token = member_jwt();
        let req = || {
            Request::builder()
                .method("POST")
                .uri(SPEC_DOC_URI)
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };

        // Pending spec → deny.
        let resp = app.clone().oneshot(req()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "resource.approval_status == pending must reach live evaluation"
        );

        // Approve the spec → deny no longer matches → Developer allow grants.
        let mut entry = state
            .spec_ledger
            .find_by_path("system/design.md")
            .await
            .unwrap()
            .expect("seeded entry");
        entry.approval_status = gyre_domain::ApprovalStatus::Approved;
        state.spec_ledger.save(&entry).await.unwrap();

        let resp = app.oneshot(req()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "approved spec must not trip the pending-only deny"
        );
    }

    /// `resource.tenant_id` must resolve through the entity's scope chain
    /// (spec → repo → workspace → tenant) for a resource whose route does
    /// not name any scope ids. The dynamic-reference form
    /// (`$subject.tenant_id`) must compare against it.
    ///
    /// Discriminating form: a Deny at priority 810 (above the builtin
    /// developer-write-access Allow at 800) keyed on cross-tenant mismatch.
    /// `member_jwt()` carries no `tenant_id` claim → `subject.tenant_id`
    /// is "default", while the seeded chain resolves the spec's tenant to
    /// t-1 — so the deny must match. A Deny (not a same-tenant Allow) is
    /// required here: a non-matching Allow would fall through to the
    /// builtin Developer Allow at 800, which grants read regardless of
    /// tenant, masking both a correct lookup and a fabricated one.
    #[tokio::test(flavor = "multi_thread")]
    async fn spec_resource_tenant_resolves_via_scope_chain() {
        let state = setup_entity_state().await;

        // Deny spec reads when the resource's tenant differs from the
        // subject's tenant (cross-tenant read → deny, outranking the
        // builtin Developer read allow at 800).
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-tenant-mismatch-deny"),
                name: "cross-tenant-read-deny".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.tenant_id".to_string(),
                    operator: ConditionOp::NotEquals,
                    value: ConditionValue::String("$subject.tenant_id".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["spec".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(SPECS_PATH_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        // member_jwt's tenant is "default" (no tenant_id claim); the seeded
        // chain uses tenant t-1, so the cross-tenant deny must match → 403.
        // (This also proves the tenant is really looked up, not fabricated
        // from the path or a default: a fabricated "default" would make the
        // NotEquals fail and the builtin Developer Allow at 800 would grant.)
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(SPEC_DOC_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "resource.tenant_id must resolve to t-1 (≠ subject tenant), not a fabricated default"
        );
    }

    /// Task/agent/mr/team resource attributes must come from the entity's
    /// own workspace (§Attributes, source "Entity lookup"): the route names
    /// only the entity, and the workspace/tenant are looked up.
    #[tokio::test(flavor = "multi_thread")]
    async fn task_entity_scope_backfills_workspace_and_tenant() {
        use gyre_domain::Task;

        let state = setup_entity_state().await;

        // A task in ws-e (created via Task::new then re-scoped), proving the
        // workspace is read from the entity, not from the path.
        let mut task = Task::new(Id::new("task-e"), "Entity lookup task", 0);
        task.workspace_id = Id::new("ws-e");
        task.repo_id = Id::new("repo-e");
        state.tasks.create(&task).await.unwrap();

        // Deny task reads when the resource tenant is t-1.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-task-tenant-deny"),
                name: "task-t1-no-read".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "resource.tenant_id".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("t-1".to_string()),
                }],
                actions: vec!["read".to_string()],
                resource_types: vec!["task".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route("/api/v1/tasks/:id", get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks/task-e")
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "task → workspace → tenant lookup must surface resource.tenant_id == t-1"
        );
    }

    /// Persona entity attributes: owner + approval status from the persona
    /// record, workspace/tenant from its scope.
    #[tokio::test(flavor = "multi_thread")]
    async fn persona_entity_attributes_reach_live_evaluation() {
        use gyre_domain::{Persona, PersonaApprovalStatus, PersonaScope};

        let state = setup_entity_state().await;

        let mut persona = Persona::new(
            Id::new("persona-e"),
            "security",
            "security",
            PersonaScope::Workspace(Id::new("ws-e")),
            "prompt",
            0,
        );
        persona.owner = Some("user:alice".to_string());
        persona.approval_status = PersonaApprovalStatus::Pending;
        state.personas.create(&persona).await.unwrap();

        // Deny persona reads while approval is pending and the caller is not
        // the owner. member_jwt's subject is member-user; the owner is
        // user:alice → deny must fire.
        state
            .policies
            .create(&Policy {
                id: gyre_common::Id::new("test-persona-deny"),
                name: "pending-persona-owner-only".to_string(),
                description: String::new(),
                scope: PolicyScope::Tenant,
                scope_id: None,
                priority: 810,
                effect: PolicyEffect::Deny,
                conditions: vec![
                    Condition {
                        attribute: "resource.approval_status".to_string(),
                        operator: ConditionOp::Equals,
                        value: ConditionValue::String("pending".to_string()),
                    },
                    Condition {
                        attribute: "resource.owner".to_string(),
                        operator: ConditionOp::NotEquals,
                        value: ConditionValue::String("member-user".to_string()),
                    },
                ],
                actions: vec!["read".to_string()],
                resource_types: vec!["persona".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: Id::new("system"),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        init_resolver();
        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let app = Router::new()
            .route(PERSONAS_ID_ROUTE, get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                abac_middleware,
            ))
            .with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(PERSONA_E_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "persona owner (user:alice) + pending approval must reach live evaluation"
        );

        // Approve the persona → the pending-only deny arm no longer matches
        // → Developer read allow (800) grants.
        let mut updated = persona;
        updated.approval_status = PersonaApprovalStatus::Approved;
        state.personas.update(&updated).await.unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(PERSONA_E_URI)
                    .header("Authorization", format!("Bearer {}", member_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "approved persona must not trip the pending-only deny"
        );
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
