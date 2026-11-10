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
    /// Arbitrary key-value facets (status, priority, assigned_to, etc.)
    pub facets: HashMap<String, String>,
}

/// Parameters for a search query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub entity_type: Option<String>,
    pub workspace_id: Option<String>,
    pub limit: usize,
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
    /// Autocomplete: entity titles beginning with `prefix`, case-insensitive,
    /// at most `limit` results (spec §API: `GET /search/suggest`).
    async fn suggest(&self, prefix: &str, limit: usize) -> Result<Vec<SearchSuggestion>>;
    /// Aggregate facet counts over the index (spec §API `GET /search/facets`
    /// and §Response Format `facet_counts`). `Some(query)` restricts the
    /// aggregation to that query's filtered match set (text terms plus
    /// `entity_type`/`workspace_id` filters, ignoring `limit`); `None`
    /// aggregates the entire index.
    async fn facet_counts(&self, query: Option<SearchQuery>) -> Result<FacetCounts>;
}
