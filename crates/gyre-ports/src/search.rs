//! Search port — full-text search across all entities.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// A document to be indexed for search.
#[derive(Debug, Clone)]
pub struct SearchDocument {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub body: String,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    /// Tenant this document belongs to (search.md §Access Scoping step 1).
    /// Callers that already know the scope set it; storage-backed adapters
    /// derive it from the workspace's scope row when absent. Documents with
    /// no resolvable tenant are visible to no tenant-scoped query.
    pub tenant_id: Option<String>,
    /// Arbitrary key-value facets (status, priority, assigned_to, etc.)
    pub facets: HashMap<String, String>,
}

/// Parameters for a search query.
///
/// Scope fields (`tenant_id`, `accessible_workspace_ids`,
/// `accessible_repo_ids`) carry the caller's resolved access (search.md
/// §Access Scoping). Adapters MUST apply them inside the index query itself
/// (SQL WHERE / query filter), never as a post-filter, so that result counts
/// cannot leak the existence of inaccessible documents. `entity_type` and
/// `workspace_id` are additional user-supplied narrowing filters, applied
/// only within the accessible set — a `workspace_id` outside the accessible
/// set yields zero results.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub entity_type: Option<String>,
    pub workspace_id: Option<String>,
    pub limit: usize,
    /// Tenant boundary for the query — every result must belong to this
    /// tenant.
    pub tenant_id: String,
    /// Workspaces the caller may see. `None` = unscoped within the tenant
    /// (Admin). `Some(set)` restricts results to those workspaces; an empty
    /// set yields zero results.
    pub accessible_workspace_ids: Option<Vec<String>>,
    /// Repos the caller may see within the accessible workspaces. `None` =
    /// all. `Some(set)` also admits repo-less (workspace-level) documents.
    pub accessible_repo_ids: Option<Vec<String>>,
}

/// A single search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    /// Short snippet of matching context.
    pub snippet: String,
    pub score: f64,
    pub facets: HashMap<String, String>,
}

#[async_trait]
pub trait SearchPort: Send + Sync {
    /// Index (or re-index) a document.
    async fn index(&self, doc: SearchDocument) -> Result<()>;
    /// Execute a search query.
    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>>;
    /// Remove a document from the index.
    async fn delete(&self, entity_type: &str, entity_id: &str) -> Result<()>;
    /// Rebuild the entire index from scratch. Returns number of documents indexed.
    async fn reindex_all(&self) -> Result<u64>;
}
