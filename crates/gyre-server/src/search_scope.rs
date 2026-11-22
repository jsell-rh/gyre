//! Search access-scope derivation (search.md §Access Scoping).
//!
//! Every search query — REST (`GET /api/v1/search`) and MCP (`gyre_search`)
//! — is filtered by the caller's access before it reaches the index:
//!
//! 1. `tenant_id` from the auth context;
//! 2. the workspaces the caller has membership in (or, for agents, the
//!    workspace/repo scope carried by their JWT claims or agent record);
//! 3. the repos within those workspaces the caller can access.
//!
//! The resolved scope is handed to `SearchPort::search` as id lists; the
//! adapters apply it inside the index query (SQL WHERE clauses), never as a
//! post-filter, so result counts cannot leak inaccessible documents. Scope
//! derivation lives server-side (hexagonal boundary: the port receives
//! resolved id lists, it never re-derives authorization).

use crate::auth::AuthenticatedAgent;
use crate::AppState;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::UserRole;
use gyre_ports::search::{
    FacetCounts, SearchDocument, SearchPort, SearchQuery, SearchResult, SearchSuggestion,
};
use gyre_ports::WorkspaceRepository;
use std::sync::Arc;

/// The caller's resolved search scope (§Access Scoping steps 1–3).
#[derive(Debug, Clone)]
pub struct SearchScope {
    /// Tenant boundary — every result must belong to this tenant.
    pub tenant_id: String,
    /// `None` = unscoped within the tenant (Admin). `Some(set)` restricts
    /// results to those workspaces; an empty set yields zero results.
    pub accessible_workspace_ids: Option<Vec<String>>,
    /// `None` = all repos within the accessible workspaces. `Some(set)`
    /// restricts to those repos (repo-less workspace docs stay visible).
    pub accessible_repo_ids: Option<Vec<String>>,
}

/// True when the caller authenticated via a Gyre-minted agent JWT — the
/// claims carry a non-empty fine-grained `scope` list (platform-model.md §1
/// Token Scoping). Global tokens, API keys, and Keycloak JWTs carry no such
/// list. Mirrors the MCP-side `is_agent_jwt` (array form); the legacy
/// `scope: "agent"` string form is accepted for tokens minted before §1.
fn is_agent_jwt(auth: &AuthenticatedAgent) -> bool {
    let Some(scope) = auth.jwt_claims.as_ref().and_then(|c| c.get("scope")) else {
        return false;
    };
    match scope {
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::String(s) => s == "agent",
        _ => false,
    }
}

/// Claim helper: string value of `key` in the caller's JWT claims.
fn claim_str(claims: &serde_json::Value, key: &str) -> Option<String> {
    claims.get(key)?.as_str().map(|s| s.to_string())
}

/// Derive the search scope for the authenticated caller.
///
/// - **Admin** (including the global dev token): unscoped within the tenant.
/// - **Agent JWT** (OIDC claims, platform-model.md §3): workspace
///   orchestrators see their whole workspace; repo orchestrators see their
///   single repo; worker agents fall through to their agent record.
/// - **Human users** (API key or Keycloak JWT): the workspaces they have an
///   accepted membership in.
/// - **Agents without JWT scope claims** (legacy UUID tokens): their agent
///   record's workspace (and repo, when bound).
///
/// Every non-admin path fails closed: an unknown caller or a failed
/// membership lookup yields an empty accessible set (zero results), never
/// unscoped access.
pub async fn derive_search_scope(state: &AppState, auth: &AuthenticatedAgent) -> SearchScope {
    let tenant_id = auth.tenant_id.clone();

    // Admin callers (incl. the global dev token) search across the tenant.
    if auth.roles.contains(&UserRole::Admin) {
        return SearchScope {
            tenant_id,
            accessible_workspace_ids: None,
            accessible_repo_ids: None,
        };
    }

    // Agent JWTs carry their scope as OIDC claims (§Access Scoping).
    if is_agent_jwt(auth) {
        if let Some(claims) = &auth.jwt_claims {
            match claims.get("orchestrator_type").and_then(|t| t.as_str()) {
                // Workspace orchestrator: all repos in its workspace,
                // nothing outside the workspace.
                Some("workspace_orchestrator") => {
                    if let Some(ws) = claim_str(claims, "workspace_id") {
                        return SearchScope {
                            tenant_id,
                            accessible_workspace_ids: Some(vec![ws]),
                            accessible_repo_ids: None,
                        };
                    }
                }
                // Repo orchestrator: its single repo within its workspace.
                Some("repo_orchestrator") => {
                    if let (Some(ws), Some(repo)) = (
                        claim_str(claims, "workspace_id"),
                        claim_str(claims, "repo_id"),
                    ) {
                        return SearchScope {
                            tenant_id,
                            accessible_workspace_ids: Some(vec![ws]),
                            accessible_repo_ids: Some(vec![repo]),
                        };
                    }
                }
                _ => {}
            }
            // Worker agent (§1 Token Scoping): workers carry `workspace_id` +
            // `repo_id` claims on their JWT — the signed claims are the
            // scope. The agent record must still exist: an unknown caller
            // fails closed even with a signed token (a deleted agent whose
            // token is still live is an inconsistent state, never widened
            // access). Pre-§1 worker tokens (no hierarchy claims) fall
            // through to the agent record below.
            if let Some(ws) = claim_str(claims, "workspace_id") {
                let repo = claim_str(claims, "repo_id");
                let known = matches!(
                    state.agents.find_by_id(&Id::new(&auth.agent_id)).await,
                    Ok(Some(_))
                );
                return if known {
                    SearchScope {
                        tenant_id,
                        accessible_workspace_ids: Some(vec![ws]),
                        accessible_repo_ids: repo.map(|r| vec![r]),
                    }
                } else {
                    SearchScope {
                        tenant_id,
                        accessible_workspace_ids: Some(vec![]),
                        accessible_repo_ids: Some(vec![]),
                    }
                };
            }
        }
    }

    // Human users: workspace membership (accepted invitations only).
    if let Some(user_id) = &auth.user_id {
        let memberships = state
            .workspace_memberships
            .list_by_user(user_id)
            .await
            .unwrap_or_default();
        let workspace_ids = memberships
            .into_iter()
            .filter(|m| m.accepted)
            .map(|m| m.workspace_id.to_string())
            .collect::<Vec<_>>();
        return SearchScope {
            tenant_id,
            accessible_workspace_ids: Some(workspace_ids),
            accessible_repo_ids: None,
        };
    }

    // Agent without JWT scope claims (legacy UUID token): its agent record.
    match state.agents.find_by_id(&Id::new(&auth.agent_id)).await {
        Ok(Some(agent)) => SearchScope {
            tenant_id,
            accessible_workspace_ids: Some(vec![agent.workspace_id.to_string()]),
            accessible_repo_ids: agent.repo_id.map(|r| vec![r.to_string()]),
        },
        // Unknown caller: fail closed — no results, never unscoped access.
        _ => SearchScope {
            tenant_id,
            accessible_workspace_ids: Some(vec![]),
            accessible_repo_ids: Some(vec![]),
        },
    }
}

/// Wrap a search port so every indexed document carries its tenant scope.
///
/// Storage-backed adapters derive a document's tenant from its workspace row
/// at index time; the mem fallback has no workspace table to consult, so the
/// tenant is resolved here (from the workspace's real scope row) and carried
/// on the document before it reaches the inner port. A workspace that does
/// not resolve leaves the tenant unset — the document is then visible to no
/// tenant-scoped query, never mis-attributed to a fabricated scope.
///
/// Wrapping the port itself (rather than resolving at individual index
/// callsites) means every write path — REST handlers, the search-sync
/// repository decorators, background jobs, the full-reindex coordinator —
/// resolves the tenant uniformly, with no per-callsite discipline to forget.
pub fn with_tenant_resolution(
    inner: Arc<dyn SearchPort>,
    workspaces: Arc<dyn WorkspaceRepository>,
) -> Arc<dyn SearchPort> {
    Arc::new(TenantResolvingSearchPort { inner, workspaces })
}

struct TenantResolvingSearchPort {
    inner: Arc<dyn SearchPort>,
    workspaces: Arc<dyn WorkspaceRepository>,
}

#[async_trait]
impl SearchPort for TenantResolvingSearchPort {
    async fn index(&self, mut doc: SearchDocument) -> anyhow::Result<()> {
        let unset = doc.tenant_id.as_deref().is_none_or(|t| t.is_empty());
        if unset {
            if let Some(ws) = doc.workspace_id.as_deref() {
                if !ws.is_empty() {
                    if let Ok(Some(ws_row)) = self.workspaces.find_by_id(&Id::new(ws)).await {
                        doc.tenant_id = Some(ws_row.tenant_id.to_string());
                    }
                }
            }
        }
        self.inner.index(doc).await
    }

    async fn search(&self, query: SearchQuery) -> anyhow::Result<Vec<SearchResult>> {
        self.inner.search(query).await
    }

    async fn delete(&self, entity_type: &str, entity_id: &str) -> anyhow::Result<()> {
        self.inner.delete(entity_type, entity_id).await
    }

    async fn reindex_all(&self) -> anyhow::Result<u64> {
        self.inner.reindex_all().await
    }

    async fn suggest(&self, query: SearchQuery) -> anyhow::Result<Vec<SearchSuggestion>> {
        self.inner.suggest(query).await
    }

    async fn facet_counts(&self, query: SearchQuery) -> anyhow::Result<FacetCounts> {
        self.inner.facet_counts(query).await
    }
}
