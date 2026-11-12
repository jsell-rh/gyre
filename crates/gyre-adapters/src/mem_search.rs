//! In-memory search adapter — query-language matching for mem-backed
//! deployments (dev/test wiring). Implements the same §Query Language
//! contract as the persistent adapters: AND-of-terms, exact quoted phrases,
//! facet filters (AND across keys, OR within a key), and since:/until:
//! bounds — case-insensitive substring matching in place of tokenized FTS.

use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::search::{
    FacetCounts, FacetFilter, SearchDocument, SearchPort, SearchQuery, SearchResult,
    SearchSuggestion, TimeBounds,
};
use std::{collections::HashMap, sync::Mutex};

/// Current unix time in seconds (document timestamps for since:/until:).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Extract a short snippet containing a match of `needle` within `text`.
fn make_snippet(text: &str, needle: &str, max_len: usize) -> String {
    let text_lower = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    if !needle.is_empty() {
        if let Some(pos) = text_lower.find(&needle_lower) {
            let start = pos.saturating_sub(40);
            let end = (pos + needle.len() + 60).min(text.len());
            let snippet = &text[start..end];
            if start > 0 {
                return format!("...{}", snippet);
            }
            return snippet.to_string();
        }
    }
    text.chars().take(max_len).collect()
}

/// Score a document against query terms and phrases.
/// Returns Some(score) if every term AND every phrase matches, None otherwise.
fn score_doc(doc: &SearchDocument, terms: &[String], phrases: &[String]) -> Option<f64> {
    let title_lower = doc.title.to_lowercase();
    let body_lower = doc.body.to_lowercase();

    let mut score = 0.0f64;
    // Phrases: exact (case-insensitive) substring — not individual terms.
    for phrase in phrases {
        let p = phrase.to_lowercase();
        if title_lower.contains(&p) {
            score += 3.0;
        } else if body_lower.contains(&p) {
            score += 2.0;
        } else {
            return None;
        }
    }
    for term in terms {
        let t = term.to_lowercase();
        let in_title = title_lower.contains(&t);
        let in_body = body_lower.contains(&t);
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

/// Whether a document passes the query's filter set: entity_type/workspace
/// shortcuts, facet filters (AND across, OR within), and time bounds.
fn doc_matches_filters(
    doc: &SearchDocument,
    entity_type: &Option<String>,
    workspace_id: &Option<String>,
    filters: &[FacetFilter],
    time: &TimeBounds,
) -> bool {
    if let Some(et) = entity_type {
        if &doc.entity_type != et {
            return false;
        }
    }
    if let Some(ws) = workspace_id {
        if doc.workspace_id.as_deref() != Some(ws.as_str()) {
            return false;
        }
    }
    for filter in filters {
        match filter {
            FacetFilter::EntityType(values) => {
                if !values.contains(&doc.entity_type) {
                    return false;
                }
            }
            FacetFilter::Workspace(values) => match &doc.workspace_id {
                Some(ws) if values.contains(ws) => {}
                _ => return false,
            },
            FacetFilter::Repo(values) => match &doc.repo_id {
                Some(repo) if values.contains(repo) => {}
                _ => return false,
            },
            FacetFilter::Metadata { key, values } => match doc.facets.get(key) {
                Some(value) if values.contains(value) => {}
                _ => return false,
            },
        }
    }
    let ts = doc.updated_at.unwrap_or(0);
    if let Some(since) = time.since {
        if ts < since {
            return false;
        }
    }
    if let Some(until) = time.until {
        if ts > until {
            return false;
        }
    }
    true
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
        let mut doc = doc;
        // Stamp index time when the indexer did not supply a timestamp —
        // since:/until: always filter on a real value.
        if doc.updated_at.is_none() {
            doc.updated_at = Some(now_secs());
        }
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
        let parsed = &query.parsed;
        let time = parsed.time_bounds(now_secs());
        let filters = parsed.facet_filters();
        let has_text = parsed.has_text();
        let has_filter = query.entity_type.is_some()
            || query.workspace_id.is_some()
            || !filters.is_empty()
            || time.since.is_some()
            || time.until.is_some();
        // Nothing to match and nothing to filter on: no results (parity with
        // the SQL adapters — an empty query never means "everything").
        if !has_text && !has_filter {
            return Ok(vec![]);
        }
        // Facet-only queries browse: every filter-passing doc matches.
        let needle = parsed
            .terms
            .first()
            .or_else(|| parsed.phrases.first())
            .cloned()
            .unwrap_or_default();

        let mut results: Vec<(f64, SearchResult)> = docs
            .iter()
            .filter(|doc| {
                doc_matches_filters(
                    doc,
                    &query.entity_type,
                    &query.workspace_id,
                    &filters,
                    &time,
                )
            })
            .filter_map(|doc| {
                let score = if has_text {
                    score_doc(doc, &parsed.terms, &parsed.phrases)?
                } else {
                    0.0
                };
                let snippet = make_snippet(&doc.body, &needle, 120);
                Some((
                    score,
                    SearchResult {
                        entity_type: doc.entity_type.clone(),
                        entity_id: doc.entity_id.clone(),
                        title: doc.title.clone(),
                        snippet,
                        score,
                        facets: doc.facets.clone(),
                    },
                ))
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
        // Some(q): restrict to q's filtered match set — the same text + facet
        // + shortcut filters `search()` applies (q's limit ignored, since the
        // counts must describe the full filtered set). None: the whole index.
        let selected: Vec<&SearchDocument> = match &query {
            None => docs.iter().collect(),
            Some(q) => {
                let parsed = &q.parsed;
                let time = parsed.time_bounds(now_secs());
                let filters = parsed.facet_filters();
                let has_text = parsed.has_text();
                let has_filter = q.entity_type.is_some()
                    || q.workspace_id.is_some()
                    || !filters.is_empty()
                    || time.since.is_some()
                    || time.until.is_some();
                if !has_text && !has_filter {
                    return Ok(FacetCounts::new());
                }
                docs.iter()
                    .filter(|doc| {
                        doc_matches_filters(doc, &q.entity_type, &q.workspace_id, &filters, &time)
                    })
                    .filter(|doc| {
                        !has_text || score_doc(doc, &parsed.terms, &parsed.phrases).is_some()
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
            updated_at: None,
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
            .search(
                SearchQuery::parse("ABAC".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("authentication".to_string())
                    .with_entity_type(Some("task".to_string()))
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("JWT".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("QUEUE".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("new".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "New title");

        // Old body should be gone.
        let old_results = adapter
            .search(
                SearchQuery::parse("old".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .facet_counts(Some(
                SearchQuery::parse("jwt".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(1),
            ))
            .await
            .unwrap();
        assert_eq!(filtered["type"]["task"], 2);
        assert_eq!(filtered["type"]["mr"], 1);
        assert_eq!(filtered["status"]["in_progress"], 2);

        // entity_type filter restricts the aggregation.
        let mr_only = adapter
            .facet_counts(Some(
                SearchQuery::parse("jwt".to_string())
                    .with_entity_type(Some("mr".to_string()))
                    .with_workspace_id(None)
                    .with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(mr_only["type"]["mr"], 1);
        assert!(!mr_only["type"].contains_key("task"));

        // workspace filter restricts the aggregation.
        let ws_only = adapter
            .facet_counts(Some(
                SearchQuery::parse("jwt".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(Some("ws-a".to_string()))
                    .with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(ws_only["type"]["task"], 1);
        assert_eq!(ws_only["status"]["done"], 1);

        // Non-matching query aggregates nothing.
        let none = adapter
            .facet_counts(Some(
                SearchQuery::parse("zzz-nonexistent".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            ))
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    // ── §Query Language (task-154) — same contract as the SQL adapters ──────

    /// Helper: search ids for a raw query-language string.
    async fn q(adapter: &MemSearchAdapter, query: &str) -> Vec<String> {
        let mut ids: Vec<String> = adapter
            .search(SearchQuery::parse(query.to_string()).with_limit(10))
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.entity_id)
            .collect();
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn query_language_phrases_and_facets() {
        let adapter = MemSearchAdapter::new();

        let mut adjacent = make_doc(
            "task",
            "adjacent",
            "Queue rules",
            "the merge queue ordering rules",
        );
        adjacent
            .facets
            .insert("status".to_string(), "in_progress".to_string());
        adjacent
            .facets
            .insert("priority".to_string(), "high".to_string());
        let mut scattered = make_doc(
            "task",
            "scattered",
            "Merge separately",
            "merge the queue then ordering",
        );
        scattered
            .facets
            .insert("status".to_string(), "done".to_string());
        let mut mr = make_doc("mr", "mr1", "Widget merge", "widget merge queue backport");
        mr.facets.insert("status".to_string(), "open".to_string());
        mr.repo_id = Some("gyre-server".to_string());
        let mut ws_doc = make_doc("task", "wsdoc", "Widget plan", "widget roadmap");
        ws_doc.workspace_id = Some("ws-a".to_string());
        for d in [adjacent, scattered, mr, ws_doc] {
            adapter.index(d).await.unwrap();
        }

        // Quoted phrase: exact substring, not individual terms.
        assert_eq!(q(&adapter, "\"merge queue\" ordering").await, ["adjacent"]);
        // Unquoted: AND of independent terms — every doc with both words.
        assert_eq!(
            q(&adapter, "merge queue").await,
            ["adjacent", "mr1", "scattered"]
        );
        // Facet filters narrow; AND across facets, OR within a facet.
        assert_eq!(
            q(&adapter, "merge queue type:task").await,
            ["adjacent", "scattered"]
        );
        assert_eq!(
            q(&adapter, "merge queue status:in_progress").await,
            ["adjacent"]
        );
        assert_eq!(q(&adapter, "type:task type:mr status:open").await, ["mr1"]);
        assert_eq!(q(&adapter, "widget workspace:ws-a").await, ["wsdoc"]);
        assert_eq!(q(&adapter, "widget repo:gyre-server").await, ["mr1"]);
        // Facet-only query browses without text.
        assert_eq!(q(&adapter, "status:open").await, ["mr1"]);
        // Non-matching facet value: nothing, not everything.
        assert!(q(&adapter, "widget status:approved").await.is_empty());
    }

    #[tokio::test]
    async fn query_language_time_bounds() {
        let adapter = MemSearchAdapter::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let mut old = make_doc("task", "ancient", "Old doc", "ancient history keyword");
        old.updated_at = Some(now - 40 * 86_400);
        let mut recent = make_doc("task", "fresh", "New doc", "fresh news keyword");
        recent.updated_at = Some(now);
        adapter.index(old).await.unwrap();
        adapter.index(recent).await.unwrap();

        assert_eq!(q(&adapter, "keyword since:7d").await, ["fresh"]);
        assert_eq!(
            q(&adapter, "keyword since:2000-01-01").await,
            ["ancient", "fresh"]
        );
        assert!(q(&adapter, "keyword since:2999-01-01").await.is_empty());
        assert_eq!(q(&adapter, "keyword until:7d").await, ["ancient"]);
        assert_eq!(
            q(&adapter, "keyword until:2999-01-01").await,
            ["ancient", "fresh"]
        );
        assert!(q(&adapter, "keyword until:2000-01-01").await.is_empty());
    }

    #[tokio::test]
    async fn query_language_facet_counts_and_malformed_queries() {
        let adapter = MemSearchAdapter::new();
        let mut t1 = make_doc("task", "t1", "jwt one", "jwt rotation one");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        let mut t2 = make_doc("task", "t2", "jwt two", "jwt rotation two");
        t2.facets.insert("status".to_string(), "done".to_string());
        adapter.index(t1).await.unwrap();
        adapter.index(t2).await.unwrap();

        // Aggregation honors facet filters in the query.
        let counts = adapter
            .facet_counts(Some(
                SearchQuery::parse("jwt status:in_progress".to_string()).with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(counts["type"]["task"], 1);
        assert_eq!(counts["status"]["in_progress"], 1);
        assert!(!counts["status"].contains_key("done"));

        // Malformed / empty queries never mean "everything".
        assert!(q(&adapter, "").await.is_empty());
        assert!(q(&adapter, "type:").await.is_empty());
    }
}
