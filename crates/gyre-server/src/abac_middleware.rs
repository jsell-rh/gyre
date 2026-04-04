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
                RouteResourceMapping::api("/api/v1/merge-requests/:id", "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/status",
                    "merge_request",
                    Some("write"),
                ),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/comments",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/reviews",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/merge-requests/:id/diff", "merge_request", None),
                RouteResourceMapping::api(
                    "/api/v1/merge-requests/:id/attestation",
                    "merge_request",
                    None,
                ),
                RouteResourceMapping::api("/api/v1/merge-requests/:id/gates", "gate", None),
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
                RouteResourceMapping::api(
                    "/api/v1/meta-specs/:path/blast-radius",
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
                RouteResourceMapping::api("/api/v1/users/me", "user", None),
                RouteResourceMapping::api("/api/v1/users/me/agents", "agent", None),
                RouteResourceMapping::api("/api/v1/users/me/tasks", "task", None),
                RouteResourceMapping::api("/api/v1/users/me/mrs", "merge_request", None),
                // Notification endpoints use per-handler auth (HSI §2) — ABAC-exempt.
                RouteResourceMapping::exempt("/api/v1/users/me/notifications"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/dismiss"),
                RouteResourceMapping::exempt("/api/v1/notifications/:id/resolve"),
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
        // Priority 1: Default deny — lowest priority catchall.
        // Anything not explicitly allowed is denied.
        Policy {
            id: Id::new("builtin-default-deny"),
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

/// Seed built-in M34 policies into the policy store at startup. Idempotent.
pub async fn seed_builtin_policies(state: &Arc<AppState>) {
    for policy in m34_builtin_policies() {
        match state.policies.find_by_id(&policy.id.to_string()).await {
            Ok(None) => {
                if let Err(e) = state.policies.create(&policy).await {
                    tracing::warn!(
                        policy_id = %policy.id,
                        err = %e,
                        "Failed to seed built-in ABAC policy"
                    );
                } else {
                    tracing::debug!(
                        policy_id = %policy.id,
                        name = %policy.name,
                        "Seeded built-in ABAC policy"
                    );
                }
            }
            Ok(Some(_)) => {} // already exists → idempotent
            Err(e) => {
                tracing::warn!(
                    policy_id = %policy.id,
                    err = %e,
                    "Error checking built-in policy existence"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global resolver
// ---------------------------------------------------------------------------

static RESOURCE_RESOLVER: OnceLock<ResourceResolver> = OnceLock::new();

/// Initialise the global `ResourceResolver` (called once at startup).
pub fn init_resolver() {
    RESOURCE_RESOLVER.get_or_init(ResourceResolver::new);
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
    // (check-api-auth.sh catches missing entries at CI time).
    let (resource_type, action_override, exempt) =
        match RESOURCE_RESOLVER.get().and_then(|r| r.resolve(&pattern)) {
            Some(m) => (m.resource_type, m.action_override, m.exempt),
            None => return next.run(req).await,
        };

    // ABAC-exempt routes pass through immediately.
    if exempt {
        return next.run(req).await;
    }

    // Extract auth from request (splits and reconstructs).
    let method = req.method().clone();
    let (mut parts, body) = req.into_parts();

    let auth = match AuthenticatedAgent::from_request_parts(&mut parts, &state).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    let req = Request::from_parts(parts, body);

    // System token bypasses ABAC entirely.
    if auth.agent_id == "system" {
        return next.run(req).await;
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

    // Resolve action.
    let action = action_override.unwrap_or_else(|| method_to_action(&method));

    // Load policies and evaluate.
    let policies = state.policies.list().await.unwrap_or_default();
    let result = policy_engine::evaluate(policies, &ctx, action, resource_type);

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
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "insufficient permissions"})),
            )
                .into_response()
        }
    }
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state_base))
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state_base))
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
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state_base))
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

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_policies_seeded_idempotently() {
        let state = test_state();
        tokio::task::block_in_place(|| {
            let h = tokio::runtime::Handle::current();
            h.block_on(seed_builtin_policies(&state));
            h.block_on(seed_builtin_policies(&state)); // second call must be no-op
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
}
