//! In-memory search adapter — case-insensitive substring match for MVP.

use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::search::{
    FacetCounts, SearchDocument, SearchPort, SearchQuery, SearchResult, SearchSuggestion,
};
use std::{collections::HashMap, sync::Mutex};

/// Extract a short snippet containing a match of `query` within `text`.
fn make_snippet(text: &str, query: &str, max_len: usize) -> String {
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    if let Some(pos) = text_lower.find(&query_lower) {
        let start = pos.saturating_sub(40);
        let end = (pos + query.len() + 60).min(text.len());
        let snippet = &text[start..end];
        if start > 0 {
            format!("...{}", snippet)
        } else {
            snippet.to_string()
        }
    } else {
        text.chars().take(max_len).collect()
    }
}

/// Score a document against a list of query terms.
/// Returns Some(score) if all terms match, None otherwise.
fn score_doc(doc: &SearchDocument, terms: &[&str]) -> Option<f64> {
    let title_lower = doc.title.to_lowercase();
    let body_lower = doc.body.to_lowercase();

    let mut score = 0.0f64;
    for term in terms {
        let in_title = title_lower.contains(term);
        let in_body = body_lower.contains(term);
        if !in_title && !in_body {
            return None;
        }
        if in_title {
            score += 2.0;
        }
        if in_body {
            score += 1.0;
        }
    }
    Some(score)
}

#[derive(Default)]
pub struct MemSearchAdapter {
    docs: Mutex<Vec<SearchDocument>>,
}

impl MemSearchAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SearchPort for MemSearchAdapter {
    async fn index(&self, doc: SearchDocument) -> Result<()> {
        let mut docs = self.docs.lock().unwrap();
        // Replace existing entry for the same entity, or insert.
        if let Some(existing) = docs
            .iter_mut()
            .find(|d| d.entity_type == doc.entity_type && d.entity_id == doc.entity_id)
        {
            *existing = doc;
        } else {
            docs.push(doc);
        }
        Ok(())
    }

    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        let docs = self.docs.lock().unwrap();
        let raw_query = query.query.to_lowercase();
        // Split into terms (simple whitespace tokenisation).
        let terms: Vec<&str> = raw_query.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(vec![]);
        }

        let mut results: Vec<(f64, SearchResult)> = docs
            .iter()
            .filter(|doc| {
                // Entity type filter.
                if let Some(ref et) = query.entity_type {
                    if doc.entity_type != *et {
                        return false;
                    }
                }
                // Workspace filter.
                if let Some(ref ws) = query.workspace_id {
                    if doc.workspace_id.as_deref() != Some(ws.as_str()) {
                        return false;
                    }
                }
                true
            })
            .filter_map(|doc| {
                score_doc(doc, &terms).map(|score| {
                    let snippet = make_snippet(&doc.body, &raw_query, 120);
                    (
                        score,
                        SearchResult {
                            entity_type: doc.entity_type.clone(),
                            entity_id: doc.entity_id.clone(),
                            title: doc.title.clone(),
                            snippet,
                            score,
                            facets: doc.facets.clone(),
                        },
                    )
                })
            })
            .collect();

        // Sort by descending score.
        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(query.limit);
        Ok(results.into_iter().map(|(_, r)| r).collect())
    }

    async fn delete(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        let mut docs = self.docs.lock().unwrap();
        docs.retain(|d| !(d.entity_type == entity_type && d.entity_id == entity_id));
        Ok(())
    }

    async fn reindex_all(&self) -> Result<u64> {
        // In-memory adapter has nothing to rebuild — index is always current.
        let count = self.docs.lock().unwrap().len() as u64;
        Ok(count)
    }

    async fn suggest(&self, prefix: &str, limit: usize) -> Result<Vec<SearchSuggestion>> {
        let prefix = prefix.trim().to_lowercase();
        if prefix.is_empty() {
            return Ok(vec![]);
        }
        let docs = self.docs.lock().unwrap();
        let mut suggestions: Vec<SearchSuggestion> = docs
            .iter()
            // Case-insensitive prefix match on the title — same contract as the
            // SQL adapters' `title LIKE 'prefix%'`.
            .filter(|d| d.title.to_lowercase().starts_with(&prefix))
            .map(|d| SearchSuggestion {
                text: d.title.clone(),
                entity_type: d.entity_type.clone(),
                entity_id: d.entity_id.clone(),
            })
            .collect();
        suggestions.sort_by(|a, b| a.text.cmp(&b.text));
        suggestions.truncate(limit);
        Ok(suggestions)
    }

    async fn facet_counts(&self, query: Option<SearchQuery>) -> Result<FacetCounts> {
        let docs = self.docs.lock().unwrap();
        // Some(q): restrict to q's filtered match set — same filters as
        // `search()` (every term must match, plus entity_type/workspace_id),
        // ignoring q's limit. None: the whole index.
        let selected: Vec<&SearchDocument> = match &query {
            None => docs.iter().collect(),
            Some(q) => {
                let query_lower = q.query.to_lowercase();
                let terms: Vec<&str> = query_lower.split_whitespace().collect();
                if terms.is_empty() {
                    return Ok(FacetCounts::new());
                }
                docs.iter()
                    .filter(|doc| {
                        if let Some(ref et) = q.entity_type {
                            if doc.entity_type != *et {
                                return false;
                            }
                        }
                        if let Some(ref ws) = q.workspace_id {
                            if doc.workspace_id.as_deref() != Some(ws.as_str()) {
                                return false;
                            }
                        }
                        score_doc(doc, &terms).is_some()
                    })
                    .collect()
            }
        };

        let mut counts: FacetCounts = HashMap::new();
        let mut bump = |facet: &str, value: String| {
            counts
                .entry(facet.to_string())
                .or_default()
                .entry(value)
                .and_modify(|c| *c += 1)
                .or_insert(1);
        };
        for doc in selected {
            bump("type", doc.entity_type.clone());
            if let Some(ws) = doc.workspace_id.as_deref() {
                if !ws.is_empty() {
                    bump("workspace", ws.to_string());
                }
            }
            for (key, value) in &doc.facets {
                bump(key, value.clone());
            }
        }
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_doc(entity_type: &str, id: &str, title: &str, body: &str) -> SearchDocument {
        SearchDocument {
            entity_type: entity_type.to_string(),
            entity_id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            workspace_id: None,
            repo_id: None,
            facets: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_index_and_search_roundtrip() {
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc(
                "task",
                "t1",
                "ABAC policy migration",
                "Migrate all ABAC rules",
            ))
            .await
            .unwrap();
        adapter
            .index(make_doc(
                "spec",
                "s1",
                "Identity spec",
                "JWT authentication identity",
            ))
            .await
            .unwrap();

        let results = adapter
            .search(SearchQuery {
                query: "ABAC".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "t1");
    }

    #[tokio::test]
    async fn test_entity_type_filter() {
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc(
                "task",
                "t1",
                "auth task",
                "authentication details",
            ))
            .await
            .unwrap();
        adapter
            .index(make_doc(
                "spec",
                "s1",
                "auth spec",
                "authentication details",
            ))
            .await
            .unwrap();

        let results = adapter
            .search(SearchQuery {
                query: "authentication".to_string(),
                entity_type: Some("task".to_string()),
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_type, "task");
    }

    #[tokio::test]
    async fn test_empty_results() {
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc("task", "t1", "ABAC migration", "some body"))
            .await
            .unwrap();

        let results = adapter
            .search(SearchQuery {
                query: "JWT".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_substring_matching_case_insensitive() {
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc(
                "mr",
                "mr1",
                "Fix merge queue",
                "Queue ordering issue",
            ))
            .await
            .unwrap();

        let results = adapter
            .search(SearchQuery {
                query: "QUEUE".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_re_index_replaces_existing() {
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc("task", "t1", "Old title", "old body"))
            .await
            .unwrap();
        adapter
            .index(make_doc(
                "task",
                "t1",
                "New title",
                "new body with search term",
            ))
            .await
            .unwrap();

        let results = adapter
            .search(SearchQuery {
                query: "new".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "New title");

        // Old body should be gone.
        let old_results = adapter
            .search(SearchQuery {
                query: "old".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(old_results.is_empty());
    }

    #[tokio::test]
    async fn test_suggest_prefix_contract() {
        // Same port contract as the SQL adapters: case-insensitive prefix on
        // the title, mid-title terms excluded, limit truncates, empty prefix
        // yields nothing (mem adapter guards in code — it has no schema).
        let adapter = MemSearchAdapter::new();
        adapter
            .index(make_doc(
                "spec",
                "s1",
                "Identity security",
                "spiffe identity",
            ))
            .await
            .unwrap();
        adapter
            .index(make_doc(
                "task",
                "t1",
                "identity provider audit",
                "oidc audit",
            ))
            .await
            .unwrap();
        adapter
            .index(make_doc("task", "t2", "Rotate keys", "jwt rotation"))
            .await
            .unwrap();

        let sugg = adapter.suggest("IDEN", 10).await.unwrap();
        assert_eq!(sugg.len(), 2, "case-insensitive prefix: {sugg:?}");
        assert_eq!(sugg[0].text, "Identity security");
        assert_eq!(sugg[0].entity_type, "spec");
        assert_eq!(sugg[0].entity_id, "s1");
        assert_eq!(sugg[1].text, "identity provider audit");

        // Mid-title term: not a prefix.
        let mid = adapter.suggest("security", 10).await.unwrap();
        assert!(mid.is_empty(), "mid-title term must not match: {mid:?}");

        // Limit truncates.
        assert_eq!(adapter.suggest("iden", 1).await.unwrap().len(), 1);

        // Empty / whitespace-only prefix: no suggestions.
        assert!(adapter.suggest("", 10).await.unwrap().is_empty());
        assert!(adapter.suggest("   ", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_facet_counts_contract() {
        // Same port contract as the SQL adapters: None aggregates the whole
        // index, Some(q) aggregates q's filtered match set (terms + filters,
        // limit ignored).
        let adapter = MemSearchAdapter::new();

        let mut t1 = make_doc("task", "t1", "jwt one", "jwt jwt jwt rotation");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        adapter.index(t1).await.unwrap();

        let mut t2 = make_doc("task", "t2", "jwt two", "jwt rotation");
        t2.facets.insert("status".to_string(), "done".to_string());
        t2.workspace_id = Some("ws-a".to_string());
        adapter.index(t2).await.unwrap();

        let mut m1 = make_doc("mr", "m1", "jwt merge", "jwt handling");
        m1.facets
            .insert("status".to_string(), "in_progress".to_string());
        adapter.index(m1).await.unwrap();

        adapter
            .index(make_doc("task", "t3", "dns fix", "dns resolver"))
            .await
            .unwrap();

        let all = adapter.facet_counts(None).await.unwrap();
        assert_eq!(all["type"]["task"], 3);
        assert_eq!(all["type"]["mr"], 1);
        assert_eq!(all["workspace"]["ws-a"], 1);
        assert_eq!(all["status"]["in_progress"], 2);
        assert_eq!(all["status"]["done"], 1);

        // limit=1 must be ignored — counts describe the full filtered set.
        let filtered = adapter
            .facet_counts(Some(SearchQuery {
                query: "jwt".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 1,
            }))
            .await
            .unwrap();
        assert_eq!(filtered["type"]["task"], 2);
        assert_eq!(filtered["type"]["mr"], 1);
        assert_eq!(filtered["status"]["in_progress"], 2);

        // entity_type filter restricts the aggregation.
        let mr_only = adapter
            .facet_counts(Some(SearchQuery {
                query: "jwt".to_string(),
                entity_type: Some("mr".to_string()),
                workspace_id: None,
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(mr_only["type"]["mr"], 1);
        assert!(!mr_only["type"].contains_key("task"));

        // workspace filter restricts the aggregation.
        let ws_only = adapter
            .facet_counts(Some(SearchQuery {
                query: "jwt".to_string(),
                entity_type: None,
                workspace_id: Some("ws-a".to_string()),
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(ws_only["type"]["task"], 1);
        assert_eq!(ws_only["status"]["done"], 1);

        // Non-matching query aggregates nothing.
        let none = adapter
            .facet_counts(Some(SearchQuery {
                query: "zzz-nonexistent".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            }))
            .await
            .unwrap();
        assert!(none.is_empty());
    }
}
