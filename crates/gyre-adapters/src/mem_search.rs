//! In-memory search adapter — query-language matching for mem-backed
//! deployments (dev/test wiring). Implements the same §Query Language
//! contract as the persistent adapters: AND-of-terms, exact quoted phrases,
//! facet filters (AND across keys, OR within a key), and since:/until:
//! bounds — case-insensitive substring matching in place of tokenized FTS.

use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::search::{
    FacetCounts, FacetFilter, SearchDocument, SearchPort, SearchQuery, SearchResult,
    SearchSuggestion,
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

/// §Access Scoping constraints plus the user-supplied narrowing filters
/// (entity_type/workspace shortcuts, query-language facet filters, and
/// since:/until: time bounds), as a predicate over one document — the mem
/// adapter's equivalent of the SQL adapters' WHERE clauses. Applied inside
/// the query filter, before scoring/counting, so result and facet counts
/// can never include documents the caller cannot access (search.md
/// §Access Scoping: never a post-filter).
fn scope_visible(query: &SearchQuery, doc: &SearchDocument) -> bool {
    // Tenant boundary.
    if doc.tenant_id.as_deref() != Some(query.tenant_id.as_str()) {
        return false;
    }
    // Entity type filter.
    if let Some(ref et) = query.entity_type {
        if doc.entity_type != *et {
            return false;
        }
    }
    // Workspace filter (user-supplied narrowing — intersects with the
    // accessible set, so it can only narrow, never widen).
    if let Some(ref ws) = query.workspace_id {
        if doc.workspace_id.as_deref() != Some(ws.as_str()) {
            return false;
        }
    }
    // Accessible-workspace set: None = unscoped (Admin), an empty
    // set excludes everything.
    if let Some(ref ids) = query.accessible_workspace_ids {
        if !ids
            .iter()
            .any(|w| doc.workspace_id.as_deref() == Some(w.as_str()))
        {
            return false;
        }
    }
    // Accessible-repo set: repo-less (workspace-level) documents stay
    // visible — repo scope restricts repos, not the workspace itself.
    if let Some(ref ids) = query.accessible_repo_ids {
        let repo_ok = match doc.repo_id.as_deref() {
            None | Some("") => true,
            Some(r) => ids.iter().any(|i| i == r),
        };
        if !repo_ok {
            return false;
        }
    }
    // Query-language facet filters (AND across keys, OR within a key).
    for filter in query.parsed.facet_filters() {
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
            FacetFilter::Metadata { key, values } => match doc.facets.get(&key) {
                Some(value) if values.contains(value) => {}
                _ => return false,
            },
        }
    }
    // Time bounds (since:/until:) on the document's timestamp.
    let time = query.parsed.time_bounds(now_secs());
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
        // Replace existing entry for the same entity + scope, or insert.
        // Scope is part of the identity: a spec/file path is only unique
        // within its repo, so same-path documents in different repos must
        // coexist rather than clobber each other.
        if let Some(existing) = docs.iter_mut().find(|d| {
            d.entity_type == doc.entity_type
                && d.entity_id == doc.entity_id
                && d.workspace_id == doc.workspace_id
                && d.repo_id == doc.repo_id
        }) {
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
            .filter(|doc| scope_visible(&query, doc))
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
        // Adapter-level contract (same as the SQLite/Postgres adapters): clear
        // the index and return how many documents were dropped. The
        // rebuild-from-domain-entities half of reindex is the server
        // coordinator's job (search_indexer::reindex_all).
        let mut docs = self.docs.lock().unwrap();
        let count = docs.len() as u64;
        docs.clear();
        Ok(count)
    }

    async fn suggest(&self, query: SearchQuery) -> Result<Vec<SearchSuggestion>> {
        let prefix = query.query.trim().to_lowercase();
        if prefix.is_empty() {
            return Ok(vec![]);
        }
        let docs = self.docs.lock().unwrap();
        // §Access Scoping: the same scope predicate as `search()`, applied
        // inside the query filter — suggestions never include titles the
        // caller cannot access.
        let mut suggestions: Vec<SearchSuggestion> = docs
            .iter()
            .filter(|d| scope_visible(&query, d))
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
        suggestions.truncate(query.limit);
        Ok(suggestions)
    }

    async fn facet_counts(&self, query: SearchQuery) -> Result<FacetCounts> {
        let docs = self.docs.lock().unwrap();
        // §Access Scoping: the same scope predicate as `search()`, applied
        // before counting — facet counts never include documents the caller
        // cannot access. Blank text: no term filter, the caller's whole
        // accessible set (the /search/facets endpoint shape). Non-blank:
        // the same filtered match set `search()` uses (text terms, quoted
        // phrases, facet filters, and the entity_type/workspace shortcuts —
        // ignoring `limit`, since counts must describe the full set).
        let parsed = &query.parsed;
        let has_text = parsed.has_text();
        let selected: Vec<&SearchDocument> = docs
            .iter()
            .filter(|doc| scope_visible(&query, doc))
            .filter(|doc| !has_text || score_doc(doc, &parsed.terms, &parsed.phrases).is_some())
            .collect();

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

    const TEST_TENANT: &str = "t-test";

    fn make_doc(entity_type: &str, id: &str, title: &str, body: &str) -> SearchDocument {
        SearchDocument {
            entity_type: entity_type.to_string(),
            entity_id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            workspace_id: None,
            repo_id: None,
            tenant_id: Some(TEST_TENANT.to_string()),
            facets: HashMap::new(),
            updated_at: None,
        }
    }

    /// Tenant-bound query with no workspace/repo restriction. Built via
    /// `SearchQuery::parse` so `query` and its query-language parse stay
    /// consistent, then scoped to the test tenant (fail-closed until
    /// `with_scope` runs — the production handlers do the same).
    fn tenant_query(query: &str) -> SearchQuery {
        SearchQuery::parse(query.to_string())
            .with_limit(10)
            .with_scope(TEST_TENANT.to_string(), None, None)
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
                ..tenant_query("ABAC")
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
                ..tenant_query("authentication")
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
                ..tenant_query("JWT")
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
                ..tenant_query("QUEUE")
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
                ..tenant_query("new")
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "New title");

        // Old body should be gone.
        let old_results = adapter
            .search(SearchQuery {
                query: "old".to_string(),
                ..tenant_query("old")
            })
            .await
            .unwrap();
        assert!(old_results.is_empty());
    }

    #[tokio::test]
    async fn access_scoping_filters_inside_the_query_filter() {
        // §Access Scoping parity with the storage-backed adapters: tenant,
        // accessible-workspace and accessible-repo sets restrict the result
        // set before counts are derived; a user-supplied workspace_id cannot
        // widen access; a document with no resolvable tenant is visible to no
        // tenant-scoped query. Suggest and facet_counts enforce the same
        // scope — titles and counts never leak inaccessible documents.
        let adapter = MemSearchAdapter::new();

        let mut in_a = make_doc("task", "in-a", "needle", "needle in workspace a");
        in_a.workspace_id = Some("ws-a".to_string());
        let mut in_b = make_doc("task", "in-b", "needle", "needle in workspace b");
        in_b.workspace_id = Some("ws-b".to_string());
        adapter.index(in_a).await.unwrap();
        adapter.index(in_b).await.unwrap();
        // No tenant -> invisible to every tenant-scoped query.
        adapter
            .index(SearchDocument {
                tenant_id: None,
                ..make_doc("task", "no-tenant", "needle", "needle with no tenant")
            })
            .await
            .unwrap();

        // Scoped to ws-a: only ws-a's doc.
        let scoped = adapter
            .search(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                ..tenant_query("needle")
            })
            .await
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].entity_id, "in-a");

        // Unscoped within tenant: both tenanted docs, never the tenantless one.
        let admin = adapter.search(tenant_query("needle")).await.unwrap();
        assert_eq!(admin.len(), 2);

        // Empty accessible set: zero results.
        let none = adapter
            .search(SearchQuery {
                accessible_workspace_ids: Some(vec![]),
                ..tenant_query("needle")
            })
            .await
            .unwrap();
        assert!(none.is_empty());

        // Wrong tenant: zero results.
        let other = adapter
            .search(SearchQuery {
                tenant_id: "other-tenant".to_string(),
                ..tenant_query("needle")
            })
            .await
            .unwrap();
        assert!(other.is_empty());

        // User-supplied workspace_id cannot widen the accessible set.
        let widened = adapter
            .search(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                workspace_id: Some("ws-b".to_string()),
                ..tenant_query("needle")
            })
            .await
            .unwrap();
        assert!(widened.is_empty());

        // Repo scope: repo docs restricted, workspace-level docs kept.
        let mut r1 = make_doc("task", "r1-doc", "widget", "widget in repo one");
        r1.workspace_id = Some("ws-a".to_string());
        r1.repo_id = Some("repo-1".to_string());
        let mut r2 = make_doc("task", "r2-doc", "widget", "widget in repo two");
        r2.workspace_id = Some("ws-a".to_string());
        r2.repo_id = Some("repo-2".to_string());
        adapter.index(r1).await.unwrap();
        adapter.index(r2).await.unwrap();
        let repo_scoped = adapter
            .search(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                accessible_repo_ids: Some(vec!["repo-1".to_string()]),
                ..tenant_query("widget")
            })
            .await
            .unwrap();
        assert_eq!(repo_scoped.len(), 1);
        assert_eq!(repo_scoped[0].entity_id, "r1-doc");

        // Facet counts over the same scoped match set: workspace-b's matching
        // doc must be absent from the counts — a post-filter that still
        // leaked counts would include ws-b here.
        let facets = adapter
            .facet_counts(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                ..tenant_query("needle")
            })
            .await
            .unwrap();
        assert_eq!(facets["workspace"]["ws-a"], 1);
        assert!(
            !facets["workspace"].contains_key("ws-b"),
            "facet counts leaked workspace b: {facets:?}"
        );

        // Blank text + scope: the caller's whole accessible set — every ws-a
        // doc (needle + both widget docs), ws-b's doc stays out of the counts.
        let whole = adapter
            .facet_counts(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                ..tenant_query("")
            })
            .await
            .unwrap();
        assert_eq!(whole["workspace"]["ws-a"], 3);
        assert!(!whole["workspace"].contains_key("ws-b"));

        // Suggest carries the same scope: only ws-a's titles.
        let sugg = adapter
            .suggest(SearchQuery {
                accessible_workspace_ids: Some(vec!["ws-a".to_string()]),
                ..tenant_query("need")
            })
            .await
            .unwrap();
        assert_eq!(sugg.len(), 1, "suggest leaked titles: {sugg:?}");
        assert_eq!(sugg[0].entity_id, "in-a");
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

        let sugg = adapter
            .suggest(SearchQuery {
                query: "IDEN".to_string(),
                ..tenant_query("")
            })
            .await
            .unwrap();
        assert_eq!(sugg.len(), 2, "case-insensitive prefix: {sugg:?}");
        assert_eq!(sugg[0].text, "Identity security");
        assert_eq!(sugg[0].entity_type, "spec");
        assert_eq!(sugg[0].entity_id, "s1");
        assert_eq!(sugg[1].text, "identity provider audit");

        // Mid-title term: not a prefix.
        let mid = adapter
            .suggest(SearchQuery {
                query: "security".to_string(),
                ..tenant_query("")
            })
            .await
            .unwrap();
        assert!(mid.is_empty(), "mid-title term must not match: {mid:?}");

        // Limit truncates.
        assert_eq!(
            adapter
                .suggest(SearchQuery {
                    query: "iden".to_string(),
                    limit: 1,
                    ..tenant_query("")
                })
                .await
                .unwrap()
                .len(),
            1
        );

        // Empty / whitespace-only prefix: no suggestions.
        for blank in ["", "   "] {
            assert!(adapter
                .suggest(SearchQuery {
                    query: blank.to_string(),
                    ..tenant_query("")
                })
                .await
                .unwrap()
                .is_empty());
        }
    }

    #[tokio::test]
    async fn test_facet_counts_contract() {
        // Same port contract as the SQL adapters: a blank query aggregates
        // the caller's whole accessible set, a text query aggregates its
        // filtered match set (terms + filters, limit ignored).
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

        // Blank query: whole-accessible-set aggregation (admin shape here —
        // unscoped within the test tenant).
        let all = adapter.facet_counts(tenant_query("")).await.unwrap();
        assert_eq!(all["type"]["task"], 3);
        assert_eq!(all["type"]["mr"], 1);
        assert_eq!(all["workspace"]["ws-a"], 1);
        assert_eq!(all["status"]["in_progress"], 2);
        assert_eq!(all["status"]["done"], 1);

        // limit=1 must be ignored — counts describe the full filtered set.
        let filtered = adapter
            .facet_counts(SearchQuery {
                limit: 1,
                ..tenant_query("jwt")
            })
            .await
            .unwrap();
        assert_eq!(filtered["type"]["task"], 2);
        assert_eq!(filtered["type"]["mr"], 1);
        assert_eq!(filtered["status"]["in_progress"], 2);

        // entity_type filter restricts the aggregation.
        let mr_only = adapter
            .facet_counts(SearchQuery {
                entity_type: Some("mr".to_string()),
                ..tenant_query("jwt")
            })
            .await
            .unwrap();
        assert_eq!(mr_only["type"]["mr"], 1);
        assert!(!mr_only["type"].contains_key("task"));

        // workspace filter restricts the aggregation.
        let ws_only = adapter
            .facet_counts(SearchQuery {
                workspace_id: Some("ws-a".to_string()),
                ..tenant_query("jwt")
            })
            .await
            .unwrap();
        assert_eq!(ws_only["type"]["task"], 1);
        assert_eq!(ws_only["status"]["done"], 1);

        // Non-matching query aggregates nothing.
        let none = adapter
            .facet_counts(tenant_query("zzz-nonexistent"))
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    // ── §Query Language (task-154) — same contract as the SQL adapters ──────

    /// Helper: search ids for a raw query-language string.
    async fn q(adapter: &MemSearchAdapter, query: &str) -> Vec<String> {
        let mut ids: Vec<String> = adapter
            .search(
                SearchQuery::parse(query.to_string())
                    .with_limit(10)
                    .with_scope(TEST_TENANT.to_string(), None, None),
            )
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
            .facet_counts(
                SearchQuery::parse("jwt status:in_progress".to_string())
                    .with_limit(10)
                    .with_scope(TEST_TENANT.to_string(), None, None),
            )
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
