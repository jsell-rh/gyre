//! In-memory search adapter — case-insensitive substring match for MVP.

use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::search::{SearchDocument, SearchPort, SearchQuery, SearchResult};
use std::sync::Mutex;

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
                // §Access Scoping: tenant + accessible workspace/repo sets are
                // enforced inside the query filter, before scoring — the
                // result set (and any count derived from it) can never
                // include documents the caller cannot access.
                if doc.tenant_id.as_deref() != Some(query.tenant_id.as_str()) {
                    return false;
                }
                // Entity type filter.
                if let Some(ref et) = query.entity_type {
                    if doc.entity_type != *et {
                        return false;
                    }
                }
                // Workspace filter (user-supplied narrowing — intersects with
                // the accessible set, so it can only narrow, never widen).
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
                // Accessible-repo set: repo-less (workspace-level) documents
                // stay visible — repo scope restricts repos, not the workspace.
                if let Some(ref ids) = query.accessible_repo_ids {
                    let repo_ok = match doc.repo_id.as_deref() {
                        None | Some("") => true,
                        Some(r) => ids.iter().any(|i| i == r),
                    };
                    if !repo_ok {
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
        }
    }

    /// Tenant-bound query with no workspace/repo restriction.
    fn tenant_query(query: &str) -> SearchQuery {
        SearchQuery {
            query: query.to_string(),
            entity_type: None,
            workspace_id: None,
            limit: 10,
            tenant_id: TEST_TENANT.to_string(),
            accessible_workspace_ids: None,
            accessible_repo_ids: None,
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
        // tenant-scoped query.
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
    }
}
