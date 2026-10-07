//! Persistent full-text search adapter — SQLite FTS5.
//!
//! Spec: search.md §Search Index (Technology, Index Schema). The index is a real
//! FTS5 virtual table with the exact specced schema (`tokenize='porter unicode61'`,
//! tenant/workspace/repo UNINDEXED, `metadata` holding JSON facets). Relevance is
//! real bm25; snippets come from FTS5 `snippet()`, not string slicing.
//!
//! The DDL is SQLite-dialect SQL and MUST NOT move into the shared diesel
//! `migrations/` dir — that dir is embedded and executed by the PostgreSQL
//! backend too (FTS5 syntax would break it). It runs here, on the SQLite-only
//! init path instead.

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Double, Nullable, Text};
use gyre_ports::search::{SearchDocument, SearchPort, SearchQuery, SearchResult};
use std::sync::Arc;

use super::SqliteStorage;

/// The specced FTS5 virtual table (§Index Schema), verbatim.
pub(crate) const FTS5_DDL: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    entity_type,
    entity_id,
    tenant_id UNINDEXED,
    workspace_id UNINDEXED,
    repo_id UNINDEXED,
    title,
    body,
    metadata,
    tokenize='porter unicode61'
);
";

/// Create the FTS5 search table on the SQLite-only init path.
pub(crate) fn ensure_fts_table(conn: &mut SqliteConnection) -> Result<()> {
    use diesel::connection::SimpleConnection;
    conn.batch_execute(FTS5_DDL)
        .context("create FTS5 search_index table")?;
    Ok(())
}

/// Build an FTS5 MATCH expression from a raw query.
///
/// Tokens are split on every non-alphanumeric character (the same token
/// boundaries the `unicode61` tokenizer uses on the index side) and joined
/// with spaces — FTS5's implicit operator between adjacent phrases is AND,
/// which is the spec's default for simple search. Raw punctuation that is
/// FTS5 query syntax (`" * - ( ) ^ :`) never reaches the parser, so no
/// user input can produce a MATCH syntax error.
fn build_match_expr(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(QueryableByName)]
struct FtsRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    entity_id: String,
    #[diesel(sql_type = Text)]
    title: String,
    /// bm25(search_index) — more negative = more relevant.
    #[diesel(sql_type = Double)]
    rank: f64,
    #[diesel(sql_type = Text)]
    snip: String,
    #[diesel(sql_type = Text)]
    metadata: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

#[derive(QueryableByName)]
struct TenantRow {
    #[diesel(sql_type = Text)]
    tenant_id: String,
}

const INSERT_SQL: &str = "
INSERT INTO search_index (entity_type, entity_id, tenant_id, workspace_id, repo_id, title, body, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
";

/// Column index of `body` inside the fts5 table (0-based), for snippet().
const BODY_COL: i32 = 6;

#[async_trait]
impl SearchPort for SqliteStorage {
    async fn index(&self, doc: SearchDocument) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let metadata_json = serde_json::to_string(&doc.facets)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;

            // Derive tenant from the workspace's real scope row (§Access Scoping);
            // empty string when the workspace is unknown — never a fabricated scope.
            let tenant_id = match doc.workspace_id.as_deref() {
                Some(ws) if !ws.is_empty() => {
                    match diesel::sql_query("SELECT tenant_id FROM workspaces WHERE id = ?1")
                        .bind::<Text, _>(ws)
                        .get_result::<TenantRow>(&mut conn)
                    {
                        Ok(r) => r.tenant_id,
                        Err(diesel::result::Error::NotFound) => String::new(),
                        Err(e) => return Err(e.into()),
                    }
                }
                _ => String::new(),
            };

            let workspace_id = doc.workspace_id.clone().unwrap_or_default();
            let repo_id = doc.repo_id.clone().unwrap_or_default();

            // Upsert: FTS5 has no ON CONFLICT — delete then insert, atomically.
            conn.transaction::<_, diesel::result::Error, _>(|tx| {
                diesel::sql_query(
                    "DELETE FROM search_index WHERE entity_type = ?1 AND entity_id = ?2",
                )
                .bind::<Text, _>(&doc.entity_type)
                .bind::<Text, _>(&doc.entity_id)
                .execute(&mut *tx)?;
                diesel::sql_query(INSERT_SQL)
                    .bind::<Text, _>(&doc.entity_type)
                    .bind::<Text, _>(&doc.entity_id)
                    .bind::<Text, _>(&tenant_id)
                    .bind::<Text, _>(&workspace_id)
                    .bind::<Text, _>(&repo_id)
                    .bind::<Text, _>(&doc.title)
                    .bind::<Text, _>(&doc.body)
                    .bind::<Text, _>(&metadata_json)
                    .execute(&mut *tx)?;
                Ok(())
            })?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        let pool = Arc::clone(&self.pool);
        Ok(tokio::task::spawn_blocking(move || -> Result<Vec<SearchResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let match_expr = build_match_expr(&query.query);
            if match_expr.is_empty() {
                return Ok(vec![]);
            }
            // entity_type / workspace_id filters are WHERE clauses evaluated by
            // the index query itself (§Access Scoping: not post-filtered).
            // bm25 ordering: best (most negative) first. snippet() wraps matches
            // in ** markers per spec §Response Format.
            let rows = diesel::sql_query(format!(
                "SELECT entity_type,
                        entity_id,
                        title,
                        bm25(search_index) AS rank,
                        snippet(search_index, {BODY_COL}, '**', '**', '…', 32) AS snip,
                        metadata
                 FROM search_index
                 WHERE search_index MATCH ?1
                   AND (?2 IS NULL OR entity_type = ?2)
                   AND (?3 IS NULL OR workspace_id = ?3)
                 ORDER BY rank
                 LIMIT ?4"
            ))
            .bind::<Text, _>(&match_expr)
            .bind::<Nullable<Text>, _>(query.entity_type)
            .bind::<Nullable<Text>, _>(query.workspace_id)
            .bind::<BigInt, _>(query.limit as i64)
            .load::<FtsRow>(&mut conn)?;

            Ok(rows
                .into_iter()
                .map(|r| {
                    // bm25 is <= 0, more negative = better. Invert and squash to
                    // (0,1] so higher score = more relevant, per spec Response Format.
                    let inverted = -r.rank;
                    let facets = serde_json::from_str(&r.metadata).unwrap_or_default();
                    SearchResult {
                        entity_type: r.entity_type,
                        entity_id: r.entity_id,
                        title: r.title,
                        snippet: r.snip,
                        score: inverted / (inverted + 1.0),
                        facets,
                    }
                })
                .collect())
        })
        .await??)
    }

    async fn delete(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let et = entity_type.to_string();
        let eid = entity_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::sql_query(
                "DELETE FROM search_index WHERE entity_type = ?1 AND entity_id = ?2",
            )
            .bind::<Text, _>(&et)
            .bind::<Text, _>(&eid)
            .execute(&mut *conn)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn reindex_all(&self) -> Result<u64> {
        // Adapter-level contract: clear the index and return how many documents
        // were dropped. The rebuild-from-domain-entities half of reindex is the
        // server coordinator's job (task-202) — not faked here.
        let pool = Arc::clone(&self.pool);
        Ok(tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::sql_query("SELECT COUNT(*) AS cnt FROM search_index")
                .get_result::<CountRow>(&mut conn)?
                .cnt;
            diesel::sql_query("INSERT INTO search_index(search_index) VALUES('delete-all')")
                .execute(&mut *conn)?;
            Ok(count as u64)
        })
        .await??)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_ports::search::SearchQuery;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(file.path().to_str().unwrap()).unwrap();
        (file, storage)
    }

    fn doc(entity_type: &str, entity_id: &str, title: &str, body: &str) -> SearchDocument {
        SearchDocument {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            workspace_id: None,
            repo_id: None,
            facets: HashMap::new(),
        }
    }

    #[derive(diesel::QueryableByName)]
    struct MasterRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        obj_type: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        sql: String,
    }

    #[tokio::test]
    async fn fts5_table_exists_with_specced_schema() {
        // §Index Schema: real FTS5 virtual table, exact columns + porter unicode61.
        let (_f, storage) = tmp_storage();
        let mut conn = storage.pool.get().unwrap();
        let row = diesel::sql_query(
            "SELECT type AS obj_type, sql FROM sqlite_master WHERE name = 'search_index'",
        )
        .get_result::<MasterRow>(&mut conn)
        .unwrap();
        assert_eq!(row.obj_type, "table");
        let sql = row.sql;
        assert!(sql.contains("fts5"), "not an fts5 table: {sql}");
        for col in [
            "entity_type",
            "entity_id",
            "tenant_id UNINDEXED",
            "workspace_id UNINDEXED",
            "repo_id UNINDEXED",
            "title",
            "body",
            "metadata",
        ] {
            assert!(sql.contains(col), "schema missing '{col}': {sql}");
        }
        assert!(
            sql.contains("tokenize='porter unicode61'"),
            "tokenizer not as specced: {sql}"
        );
    }

    #[tokio::test]
    async fn index_search_roundtrip_relevance_snippet_and_facets() {
        let (_f, storage) = tmp_storage();

        let mut jwt_facets = HashMap::new();
        jwt_facets.insert("status".to_string(), "in_progress".to_string());
        jwt_facets.insert("priority".to_string(), "high".to_string());
        let mut jwt_doc = doc(
            "task",
            "t1",
            "Rotate signing keys",
            "The jwt token rotation job refreshes jwt secrets hourly and writes jwt metrics.",
        );
        jwt_doc.facets = jwt_facets;
        storage.index(jwt_doc).await.unwrap();

        // Contains "jwt" once, plus "rotation".
        storage
            .index(doc(
                "mr",
                "m1",
                "Simplify token pipeline",
                "Adds rotation aware jwt handling in the middleware.",
            ))
            .await
            .unwrap();

        // No query term at all — must never match.
        storage
            .index(doc(
                "task",
                "t2",
                "Fix flaky dns test",
                "The dns resolver cache was stale between runs.",
            ))
            .await
            .unwrap();

        let results = storage
            .search(SearchQuery {
                query: "jwt".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 2, "unrelated doc must not match");
        // bm25 ordering: doc with three jwt occurrences ranks above the one with one.
        assert_eq!(results[0].entity_id, "t1");
        assert_eq!(results[1].entity_id, "m1");
        assert!(
            results[0].score > results[1].score,
            "bm25 ordering broken: {:?}",
            results.iter().map(|r| r.score).collect::<Vec<_>>()
        );
        assert!(
            results[0].score > 0.0 && results[0].score <= 1.0,
            "score out of (0,1]: {}",
            results[0].score
        );
        // FTS5 snippet() with ** match markers — substring matching has no markers.
        assert!(
            results[0].snippet.contains("**jwt**") || results[0].snippet.contains("**Jwt**"),
            "snippet lacks FTS5 markers: {}",
            results[0].snippet
        );
        // Facets survive the metadata JSON round-trip.
        assert_eq!(results[0].facets.get("status").map(String::as_str), Some("in_progress"));
        assert_eq!(results[0].facets.get("priority").map(String::as_str), Some("high"));
        assert!(results[1].facets.is_empty());
    }

    #[tokio::test]
    async fn and_of_terms_semantics() {
        let (_f, storage) = tmp_storage();
        storage
            .index(doc("task", "both", "a b", "alpha beta document"))
            .await
            .unwrap();
        storage
            .index(doc("task", "only_alpha", "a", "alpha only document"))
            .await
            .unwrap();

        let results = storage
            .search(SearchQuery {
                query: "alpha beta".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        // Default AND-of-terms: both terms must appear.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "both");

        // Empty query matches nothing (parity with prior adapter behavior).
        let empty = storage
            .search(SearchQuery {
                query: "".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(empty.is_empty());

        // Syntax-hostile query must return cleanly, not a MATCH parse error.
        let hostile = storage
            .search(SearchQuery {
                query: "\"unterminated ( AND -^:*".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        // "AND" is a bare token here — indexed docs don't contain it.
        assert!(hostile.is_empty());
    }

    #[tokio::test]
    async fn porters_stemming_proves_fts_not_substring() {
        // LIKE '%running%' would NOT match "runs" — porter stemming does.
        // This test fails if search() degrades to substring/LIKE matching.
        let (_f, storage) = tmp_storage();
        storage
            .index(doc("task", "t1", "Reaper", "The janitor runs every night."))
            .await
            .unwrap();
        let results = storage
            .search(SearchQuery {
                query: "running".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1, "porter stemmer must match runs~running");
    }

    #[tokio::test]
    async fn entity_type_and_workspace_filters_apply_at_index_level() {
        let (_f, storage) = tmp_storage();
        let mut d1 = doc("task", "t1", "widget plan", "widget roadmap");
        d1.workspace_id = Some("ws-a".to_string());
        let mut d2 = doc("mr", "m1", "widget merge", "widget refactor");
        d2.workspace_id = Some("ws-b".to_string());
        storage.index(d1).await.unwrap();
        storage.index(d2).await.unwrap();

        let by_type = storage
            .search(SearchQuery {
                query: "widget".to_string(),
                entity_type: Some("mr".to_string()),
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].entity_id, "m1");

        let by_ws = storage
            .search(SearchQuery {
                query: "widget".to_string(),
                entity_type: None,
                workspace_id: Some("ws-a".to_string()),
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(by_ws.len(), 1);
        assert_eq!(by_ws[0].entity_id, "t1");

        // Combined filter excluding everything.
        let none = storage
            .search(SearchQuery {
                query: "widget".to_string(),
                entity_type: Some("task".to_string()),
                workspace_id: Some("ws-b".to_string()),
                limit: 10,
            })
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn upsert_replaces_and_deletes() {
        let (_f, storage) = tmp_storage();
        storage
            .index(doc("task", "t1", "old title", "obsolete keyword zzz"))
            .await
            .unwrap();
        // Re-index same id: old body must be gone, not duplicated.
        storage
            .index(doc("task", "t1", "new title", "fresh content yyy"))
            .await
            .unwrap();

        let stale = storage
            .search(SearchQuery {
                query: "obsolete".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(stale.is_empty(), "upsert left a stale row");

        let fresh = storage
            .search(SearchQuery {
                query: "fresh".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].title, "new title");

        storage.delete("task", "t1").await.unwrap();
        let gone = storage
            .search(SearchQuery {
                query: "fresh".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(gone.is_empty());
    }

    #[tokio::test]
    async fn reindex_all_clears_and_reports_count() {
        let (_f, storage) = tmp_storage();
        storage.index(doc("task", "t1", "a", "termone text")).await.unwrap();
        storage.index(doc("task", "t2", "b", "termone text")).await.unwrap();
        assert_eq!(storage.reindex_all().await.unwrap(), 2);
        let after = storage
            .search(SearchQuery {
                query: "termone".to_string(),
                entity_type: None,
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(after.is_empty(), "index must be cleared");
        assert_eq!(storage.reindex_all().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn index_survives_storage_reopen() {
        // Durability is the point of this task: MemSearchAdapter loses everything.
        let file = NamedTempFile::new().unwrap();
        {
            let storage = SqliteStorage::new(file.path().to_str().unwrap()).unwrap();
            storage
                .index(doc("agent", "a1", "Worker", "persona orchestrator prompt"))
                .await
                .unwrap();
        }
        let storage = SqliteStorage::new(file.path().to_str().unwrap()).unwrap();
        let results = storage
            .search(SearchQuery {
                query: "orchestrator".to_string(),
                entity_type: Some("agent".to_string()),
                workspace_id: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "a1");
    }
}
