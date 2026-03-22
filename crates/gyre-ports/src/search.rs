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
