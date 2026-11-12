//! Search port — full-text search across all entities.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// The query-language types live in gyre-domain (pure logic, hexagonal
// layering: ports describes the protocol, domain owns the parsing) and are
// re-exported here so port consumers see one search module.
pub use gyre_domain::search_query::{parse_query, FacetFilter, ParsedQuery, TimeBounds};

/// Metadata-facet key under which the SQL adapters persist a document's
/// timestamp inside the specced `metadata` JSON column. The FTS5 index
/// schema (search.md §Index Schema) has no timestamp column and SQLite
/// forbids ALTER on virtual tables, so `since:`/`until:` filtering reads
/// this key via `json_extract`. Adapters strip it from result facet maps
/// and facet-count aggregations — it is storage, not a user-facing facet.
pub const UPDATED_AT_KEY: &str = "updated_at";

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
    /// Document timestamp (unix epoch seconds) the `since:`/`until:` facets
    /// filter on (search.md §Query Language). `None` — the adapter stamps
    /// index time.
    pub updated_at: Option<i64>,
}

/// Parameters for a search query.
///
/// `query` is the raw query text (echoed back in responses); `parsed` is its
/// query-language parse (search.md §Query Language) and is what the adapters
/// execute — build via [`SearchQuery::parse`] so the two stay consistent.
/// `entity_type`/`workspace_id` are backward-compatible filter shortcuts
/// equivalent to `type:{value}` / `workspace:{value}`; callers resolve
/// precedence (query-string facets win) before constructing the query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub parsed: ParsedQuery,
    pub entity_type: Option<String>,
    pub workspace_id: Option<String>,
    pub limit: usize,
}

impl SearchQuery {
    /// Build a query from raw query text, parsing it with the query
    /// language (terms, quoted phrases, facets). Default limit 20.
    pub fn parse(query: impl Into<String>) -> Self {
        let query = query.into();
        let parsed = parse_query(&query);
        Self {
            parsed,
            query,
            entity_type: None,
            workspace_id: None,
            limit: 20,
        }
    }

    /// Set the backward-compatible entity-type filter shortcut.
    pub fn with_entity_type(mut self, entity_type: Option<String>) -> Self {
        self.entity_type = entity_type;
        self
    }

    /// Set the backward-compatible workspace filter shortcut.
    pub fn with_workspace_id(mut self, workspace_id: Option<String>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    /// Set the result-page limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
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
    /// aggregation to that query's filtered match set (text terms, quoted
    /// phrases, and facet filters — including the `entity_type`/
    /// `workspace_id` shortcuts — ignoring `limit`); `None` aggregates the
    /// entire index.
    async fn facet_counts(&self, query: Option<SearchQuery>) -> Result<FacetCounts>;
}
