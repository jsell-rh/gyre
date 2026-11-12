//! Search port — full-text search across all entities.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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

/// A single autocomplete suggestion (spec §API: `GET /search/suggest`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSuggestion {
    /// The matched title text to offer in the autocomplete dropdown.
    pub text: String,
    pub entity_type: String,
    pub entity_id: String,
}

/// Aggregated facet counts, keyed `facet -> value -> count`. The built-in
/// `type` facet counts `entity_type` values and `workspace` counts
/// `workspace_id` values; every other key comes from the documents' facet
/// maps (status, priority, ...).
pub type FacetCounts = HashMap<String, HashMap<String, usize>>;

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait SearchPort: Send + Sync {
    /// Index (or re-index) a document.
    async fn index(&self, doc: SearchDocument) -> Result<()>;
    /// Execute a search query.
    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>>;
    /// Remove a document from the index.
    async fn delete(&self, entity_type: &str, entity_id: &str) -> Result<()>;
    /// Rebuild the entire index from scratch. Returns number of documents indexed.
    async fn reindex_all(&self) -> Result<u64>;
    /// Autocomplete: entity titles beginning with the `query.query` prefix,
    /// case-insensitive, at most `query.limit` results (spec §API: `GET
    /// /search/suggest`). The scope fields are enforced exactly like
    /// `search()` — suggestions never include titles of entities the caller
    /// cannot access. `entity_type`/`workspace_id` narrow within that scope.
    async fn suggest(&self, query: SearchQuery) -> Result<Vec<SearchSuggestion>>;
    /// Aggregate facet counts over the index (spec §API `GET /search/facets`
    /// and §Response Format `facet_counts`). The scope fields are enforced
    /// exactly like `search()` — counts never include documents the caller
    /// cannot access. A blank `query.query` aggregates the caller's entire
    /// accessible set; otherwise the aggregation is restricted to the same
    /// filtered match set `search()` uses (text terms plus
    /// `entity_type`/`workspace_id` filters, ignoring `limit`) — counts must
    /// describe the full filtered set, not the truncated result page.
    async fn facet_counts(&self, query: SearchQuery) -> Result<FacetCounts>;
}
