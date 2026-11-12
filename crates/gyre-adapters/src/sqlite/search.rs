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

// `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
// `Self { field: field, … }` with spans mapped onto the field declarations
// below, so clippy::redundant_field_names fires on derive-generated code.
// The generated construction sits in a dummy module that inherits neither
// struct- nor field-level `#[allow]` (verified: both still warn), so the
// lint is scoped off for this whole module instead. Every struct here is a
// `QueryableByName` row carrier — nothing else in this file writes
// `field: field` initializations by hand.
#![allow(clippy::redundant_field_names)]
use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Double, Text};
use gyre_ports::search::{
    FacetCounts, FacetFilter, SearchDocument, SearchPort, SearchQuery, SearchResult,
    SearchSuggestion, UPDATED_AT_KEY,
};
use std::{collections::HashMap, sync::Arc};

use super::SqliteStorage;

/// Current unix time in seconds (document timestamps for `since:`/`until:`).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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

/// Build an FTS5 MATCH expression from a parsed query (§Query Language).
///
/// Each term and each quoted phrase becomes an FTS5 phrase; adjacent phrases
/// imply AND — the spec's default for simple search. Quoting is what makes
/// this safe: FTS5 parses bare `AND`/`OR`/`NEAR` as boolean operators and raw
/// `" * - ( ) ^ :` as query syntax, so an unquoted token stream lets user
/// input produce MATCH syntax errors. Inside quotes every token is a literal
/// term, so no input can reach the parser as syntax. The parser guarantees
/// terms are single alphanumeric tokens and phrases contain no `"`, so both
/// can be quoted verbatim.
fn build_match_expr(parsed: &gyre_ports::search::ParsedQuery) -> String {
    let mut parts: Vec<String> = parsed.terms.iter().map(|t| format!("\"{t}\"")).collect();
    parts.extend(parsed.phrases.iter().map(|p| format!("\"{p}\"")));
    parts.join(" ")
}

/// SQL expression reading a document's timestamp out of the metadata JSON
/// (see [`UPDATED_AT_KEY`] — the FTS5 schema has no timestamp column and
/// SQLite forbids ALTER on virtual tables). Pre-migration rows have no key,
/// hence the COALESCE to 0 (epoch: excluded by `since:`, kept by `until:`
/// far-future bounds).
const TS_EXPR: &str = "COALESCE(CAST(json_extract(metadata, '$.updated_at') AS INTEGER), 0)";

/// A dynamically bound SQL parameter — the WHERE clauses vary with the
/// parsed query's facet filters, so the statement shape is built per query.
enum SqlParam {
    Text(String),
    Int(i64),
}

/// Incremental builder for the filter WHERE clause: emits numbered
/// placeholders (`?1`, `?2`, …) in bind order and collects the values.
struct WhereBuilder {
    clauses: Vec<String>,
    params: Vec<SqlParam>,
    n: usize,
}

impl WhereBuilder {
    fn new() -> Self {
        WhereBuilder {
            clauses: vec![],
            params: vec![],
            n: 0,
        }
    }

    fn push(&mut self, param: SqlParam) -> String {
        self.n += 1;
        self.params.push(param);
        format!("?{}", self.n)
    }

    fn push_text(&mut self, value: String) -> String {
        self.push(SqlParam::Text(value))
    }

    fn push_int(&mut self, value: i64) -> String {
        self.push(SqlParam::Int(value))
    }

    fn push_clause(&mut self, clause: String) {
        self.clauses.push(clause);
    }

    /// `column IN (?, …)` for one facet's values — the OR-within-facet
    /// composition (§Faceted Filtering).
    fn push_in_clause(&mut self, column: &str, values: &[String]) {
        let placeholders: Vec<String> = values.iter().map(|v| self.push_text(v.clone())).collect();
        self.push_clause(format!("{column} IN ({})", placeholders.join(", ")));
    }

    fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.clauses.join(" AND "))
        }
    }
}

/// Shared filter construction for `search()` and `facet_counts()`: MATCH
/// text plus entity_type/workspace_id shortcuts plus facet filters plus
/// `since:`/`until:` bounds, AND-composed. Returns whether any text must
/// match (facets-only queries browse without a MATCH clause).
fn build_filters(query: &SearchQuery, w: &mut WhereBuilder) -> bool {
    let parsed = &query.parsed;
    let match_expr = build_match_expr(parsed);
    let has_text = !match_expr.is_empty();
    if has_text {
        let p = w.push_text(match_expr);
        w.push_clause(format!("search_index MATCH {p}"));
    }
    if let Some(et) = &query.entity_type {
        let p = w.push_text(et.clone());
        w.push_clause(format!("entity_type = {p}"));
    }
    if let Some(ws) = &query.workspace_id {
        let p = w.push_text(ws.clone());
        w.push_clause(format!("workspace_id = {p}"));
    }
    for filter in parsed.facet_filters() {
        match filter {
            FacetFilter::EntityType(values) => w.push_in_clause("entity_type", &values),
            FacetFilter::Workspace(values) => w.push_in_clause("workspace_id", &values),
            FacetFilter::Repo(values) => w.push_in_clause("repo_id", &values),
            // Metadata facets (status/priority/label/…) live in the metadata
            // JSON column; the path is a bound parameter and the key set is
            // whitelisted by the parser, so no user text is interpolated.
            FacetFilter::Metadata { key, values } => {
                let path = w.push_text(format!("$.{key}"));
                let placeholders: Vec<String> =
                    values.iter().map(|v| w.push_text(v.clone())).collect();
                w.push_clause(format!(
                    "json_extract(metadata, {path}) IN ({})",
                    placeholders.join(", ")
                ));
            }
        }
    }
    let time = parsed.time_bounds(now_secs());
    if let Some(since) = time.since {
        let p = w.push_int(since);
        w.push_clause(format!("{TS_EXPR} >= {p}"));
    }
    if let Some(until) = time.until {
        let p = w.push_int(until);
        w.push_clause(format!("{TS_EXPR} <= {p}"));
    }
    has_text
}

// `QueryableByName`'s derive expands to `Self { field: field, … }` with spans
// mapped onto these declarations, so clippy::redundant_field_names fires on
// generated code, not on anything written here.
#[derive(QueryableByName)]
#[allow(clippy::redundant_field_names)]
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
#[allow(clippy::redundant_field_names)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

#[derive(QueryableByName)]
#[allow(clippy::redundant_field_names)]
struct TenantRow {
    #[diesel(sql_type = Text)]
    tenant_id: String,
}

/// Row carrier for prefix-suggestion queries on the title column.
#[derive(QueryableByName)]
#[allow(clippy::redundant_field_names)]
struct SuggestRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    entity_id: String,
    #[diesel(sql_type = Text)]
    title: String,
}

/// Row carrier for facet aggregation: scope columns + facets JSON.
#[derive(QueryableByName)]
#[allow(clippy::redundant_field_names)]
struct FacetRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    workspace_id: String,
    #[diesel(sql_type = Text)]
    metadata: String,
}

/// Escape SQL LIKE wildcards (`%`, `_`) and the escape character itself in a
/// user-supplied prefix so it is matched literally under `ESCAPE '\'`.
fn escape_like(prefix: &str) -> String {
    let mut escaped = String::with_capacity(prefix.len());
    for c in prefix.chars() {
        if c == '%' || c == '_' || c == '\\' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Fold one row into a `FacetCounts` aggregation: the built-in `type` facet
/// from `entity_type`, `workspace` from a non-empty `workspace_id`, and every
/// metadata facet key/value except the reserved timestamp key (storage, not
/// a user-facing facet).
fn count_facet_row(counts: &mut FacetCounts, row: FacetRow) {
    counts
        .entry("type".to_string())
        .or_default()
        .entry(row.entity_type)
        .and_modify(|c| *c += 1)
        .or_insert(1);
    if !row.workspace_id.is_empty() {
        counts
            .entry("workspace".to_string())
            .or_default()
            .entry(row.workspace_id)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }
    if let Ok(facets) = serde_json::from_str::<HashMap<String, String>>(&row.metadata) {
        for (key, value) in facets {
            if key == UPDATED_AT_KEY {
                continue;
            }
            counts
                .entry(key)
                .or_default()
                .entry(value)
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }
    }
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
        // Document timestamp for since:/until: — explicit when the indexer
        // knows the entity's update time, index time otherwise.
        let updated_at = doc.updated_at.unwrap_or_else(now_secs);
        // Persist it under the reserved metadata key: the FTS5 schema
        // (§Index Schema) has no timestamp column and SQLite forbids ALTER
        // on virtual tables.
        let mut facets = doc.facets.clone();
        facets.insert(UPDATED_AT_KEY.to_string(), updated_at.to_string());
        let metadata_json = serde_json::to_string(&facets)?;
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
        Ok(
            tokio::task::spawn_blocking(move || -> Result<Vec<SearchResult>> {
                let mut conn = pool.get().context("get db connection")?;

                // §Query Language: MATCH text + facet filters, AND-composed;
                // a facet-only query browses without a MATCH clause. All
                // filters are WHERE clauses evaluated by the index query
                // itself (§Access Scoping: not post-filtered), so the result
                // limit never truncates before filtering.
                let mut w = WhereBuilder::new();
                let has_text = build_filters(&query, &mut w);
                if !has_text && w.is_empty() {
                    return Ok(vec![]);
                }

                // bm25/snippet need a MATCH context; without one (facet-only
                // browse) rank is a constant and the snippet is a body prefix.
                let (rank_snip, order) = if has_text {
                    (
                        format!(
                            "bm25(search_index) AS rank,
                        snippet(search_index, {BODY_COL}, '**', '**', '…', 32) AS snip"
                        ),
                        "ORDER BY rank".to_string(),
                    )
                } else {
                    (
                        "0.0 AS rank,
                        substr(replace(body, char(10), ' '), 1, 120) AS snip"
                            .to_string(),
                        format!("ORDER BY {TS_EXPR} DESC"),
                    )
                };
                let limit = w.push_int(query.limit as i64);
                let sql = format!(
                    "SELECT entity_type,
                        entity_id,
                        title,
                        {rank_snip},
                        metadata
                 FROM search_index{} {order} LIMIT {limit}",
                    w.sql()
                );

                let mut stmt = diesel::sql_query(sql).into_boxed::<diesel::sqlite::Sqlite>();
                for param in w.params {
                    stmt = match param {
                        SqlParam::Text(v) => stmt.bind::<Text, _>(v),
                        SqlParam::Int(v) => stmt.bind::<BigInt, _>(v),
                    };
                }
                let rows = stmt.load::<FtsRow>(&mut conn)?;

                Ok(rows
                    .into_iter()
                    .map(|r| {
                        // bm25 is <= 0, more negative = better. Invert and squash to
                        // (0,1] so higher score = more relevant, per spec Response Format.
                        let inverted = -r.rank;
                        let mut facets: HashMap<String, String> =
                            serde_json::from_str(&r.metadata).unwrap_or_default();
                        // The reserved timestamp key is storage, not a facet.
                        facets.remove(UPDATED_AT_KEY);
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
            .await??,
        )
    }

    async fn delete(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let et = entity_type.to_string();
        let eid = entity_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::sql_query("DELETE FROM search_index WHERE entity_type = ?1 AND entity_id = ?2")
                .bind::<Text, _>(&et)
                .bind::<Text, _>(&eid)
                .execute(&mut *conn)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn suggest(&self, prefix: &str, limit: usize) -> Result<Vec<SearchSuggestion>> {
        let pool = Arc::clone(&self.pool);
        // Trimmed-empty prefixes match every title — treat them as "no
        // suggestion" (the handler guards this too; the port is safe alone).
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Ok(vec![]);
        }
        let pattern = format!("{}%", escape_like(prefix));
        Ok(
            tokio::task::spawn_blocking(move || -> Result<Vec<SearchSuggestion>> {
                let mut conn = pool.get().context("get db connection")?;
                // LIKE on the title column is ASCII-case-insensitive in SQLite —
                // a prefix match, not a token match, which is what autocomplete
                // needs (mid-word terms are unreachable from a prefix).
                let rows = diesel::sql_query(
                    "SELECT entity_type, entity_id, title
                 FROM search_index
                 WHERE title LIKE ?1 ESCAPE '\\'
                 ORDER BY title
                 LIMIT ?2",
                )
                .bind::<Text, _>(&pattern)
                .bind::<BigInt, _>(limit as i64)
                .load::<SuggestRow>(&mut conn)?;

                Ok(rows
                    .into_iter()
                    .map(|r| SearchSuggestion {
                        text: r.title,
                        entity_type: r.entity_type,
                        entity_id: r.entity_id,
                    })
                    .collect())
            })
            .await??,
        )
    }

    async fn facet_counts(&self, query: Option<SearchQuery>) -> Result<FacetCounts> {
        let pool = Arc::clone(&self.pool);
        Ok(tokio::task::spawn_blocking(move || -> Result<FacetCounts> {
            let mut conn = pool.get().context("get db connection")?;

            // Two shapes: whole-index aggregation (no WHERE) or the same
            // filtered match set `search()` uses (MATCH + facets + shortcuts),
            // without the result limit — facet counts must describe the full
            // filtered set, not the truncated page.
            let rows: Vec<FacetRow> = match query {
                None => diesel::sql_query(
                    "SELECT entity_type, workspace_id, metadata FROM search_index",
                )
                .load::<FacetRow>(&mut conn)?,
                Some(q) => {
                    let mut w = WhereBuilder::new();
                    let has_text = build_filters(&q, &mut w);
                    if !has_text && w.is_empty() {
                        return Ok(FacetCounts::new());
                    }
                    let sql = format!(
                        "SELECT entity_type, workspace_id, metadata FROM search_index{}",
                        w.sql()
                    );
                    let mut stmt = diesel::sql_query(sql).into_boxed::<diesel::sqlite::Sqlite>();
                    for param in w.params {
                        stmt = match param {
                            SqlParam::Text(v) => stmt.bind::<Text, _>(v),
                            SqlParam::Int(v) => stmt.bind::<BigInt, _>(v),
                        };
                    }
                    stmt.load::<FacetRow>(&mut conn)?
                }
            };

            let mut counts = FacetCounts::new();
            for row in rows {
                count_facet_row(&mut counts, row);
            }
            Ok(counts)
        })
        .await??)
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
            // `delete-all` is rejected on a regular content table (it requires
            // a contentless/external-content fts5 table); DELETE works on any.
            diesel::sql_query("DELETE FROM search_index").execute(&mut *conn)?;
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
            updated_at: None,
        }
    }

    #[derive(diesel::QueryableByName)]
    #[allow(clippy::redundant_field_names)]
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
            .search(
                SearchQuery::parse("jwt".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
        assert_eq!(
            results[0].facets.get("status").map(String::as_str),
            Some("in_progress")
        );
        assert_eq!(
            results[0].facets.get("priority").map(String::as_str),
            Some("high")
        );
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
            .search(
                SearchQuery::parse("alpha beta".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        // Default AND-of-terms: both terms must appear.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "both");

        // Empty query matches nothing (parity with prior adapter behavior).
        let empty = storage
            .search(
                SearchQuery::parse("".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert!(empty.is_empty());

        // Syntax-hostile query must return cleanly, not a MATCH parse error.
        let hostile = storage
            .search(
                SearchQuery::parse("\"unterminated ( AND -^:*".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        // Every token is quoted into a literal phrase — bare AND/OR/NEAR can
        // never reach the parser as a boolean operator. Docs match neither.
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
            .search(
                SearchQuery::parse("running".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("widget".to_string())
                    .with_entity_type(Some("mr".to_string()))
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].entity_id, "m1");

        let by_ws = storage
            .search(
                SearchQuery::parse("widget".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(Some("ws-a".to_string()))
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert_eq!(by_ws.len(), 1);
        assert_eq!(by_ws[0].entity_id, "t1");

        // Combined filter excluding everything.
        let none = storage
            .search(
                SearchQuery::parse("widget".to_string())
                    .with_entity_type(Some("task".to_string()))
                    .with_workspace_id(Some("ws-b".to_string()))
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn suggest_prefix_matches_titles_case_insensitively() {
        let (_f, storage) = tmp_storage();
        storage
            .index(doc(
                "spec",
                "s1",
                "Identity security",
                "spiffe workload identity",
            ))
            .await
            .unwrap();
        storage
            .index(doc("task", "t1", "Rotate signing keys", "jwt rotation job"))
            .await
            .unwrap();
        storage
            .index(doc(
                "task",
                "t2",
                "identity provider audit",
                "oidc audit trail",
            ))
            .await
            .unwrap();

        // Case-insensitive prefix on the title: lowercase query, mixed-case titles.
        let sugg = storage.suggest("iden", 10).await.unwrap();
        assert_eq!(
            sugg.len(),
            2,
            "expected both identity-prefixed titles: {sugg:?}"
        );
        // ORDER BY title: binary collation puts uppercase 'I' before lowercase 'i'.
        assert_eq!(sugg[0].text, "Identity security");
        assert_eq!(sugg[0].entity_type, "spec");
        assert_eq!(sugg[0].entity_id, "s1");
        assert_eq!(sugg[1].text, "identity provider audit");
        assert_eq!(sugg[1].entity_id, "t2");

        // A term appearing mid-title is NOT a prefix match — autocomplete must
        // not degrade to substring search.
        let mid = storage.suggest("security", 10).await.unwrap();
        assert!(
            mid.is_empty(),
            "mid-title term must not prefix-match: {mid:?}"
        );

        // Limit truncates.
        let limited = storage.suggest("iden", 1).await.unwrap();
        assert_eq!(limited.len(), 1);

        // Trimmed-empty prefix matches every title — must yield nothing.
        assert!(storage.suggest("", 10).await.unwrap().is_empty());
        assert!(storage.suggest("   ", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn suggest_escapes_like_wildcards_in_prefix() {
        let (_f, storage) = tmp_storage();
        storage
            .index(doc("task", "t1", "100% coverage task", "metrics body"))
            .await
            .unwrap();
        // Without ESCAPE, the pattern "100%%" would match this title too
        // ("%" as wildcard) — the escaped literal must not.
        storage
            .index(doc("task", "t2", "100 percent plan", "planning body"))
            .await
            .unwrap();
        // Same kill for "_": unescaped, "a_" matches any "a?" title.
        storage
            .index(doc("task", "t3", "a_b literal", "underscore body"))
            .await
            .unwrap();
        storage
            .index(doc("task", "t4", "axb wildcard", "underscore body"))
            .await
            .unwrap();

        let pct = storage.suggest("100%", 10).await.unwrap();
        assert_eq!(pct.len(), 1, "literal % must not act as wildcard: {pct:?}");
        assert_eq!(pct[0].entity_id, "t1");

        let und = storage.suggest("a_", 10).await.unwrap();
        assert_eq!(und.len(), 1, "literal _ must not act as wildcard: {und:?}");
        assert_eq!(und[0].entity_id, "t3");
    }

    #[tokio::test]
    async fn facet_counts_whole_index_and_filtered_match_set() {
        let (_f, storage) = tmp_storage();

        let mut t1 = doc("task", "t1", "jwt rotation one", "jwt jwt jwt rotation");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        t1.facets.insert("priority".to_string(), "high".to_string());
        storage.index(t1).await.unwrap();

        let mut t2 = doc("task", "t2", "jwt rotation two", "jwt rotation");
        t2.facets.insert("status".to_string(), "done".to_string());
        t2.workspace_id = Some("ws-a".to_string());
        storage.index(t2).await.unwrap();

        let mut m1 = doc("mr", "m1", "jwt merge", "jwt handling");
        m1.facets
            .insert("status".to_string(), "in_progress".to_string());
        storage.index(m1).await.unwrap();

        // No "jwt" term — never in the filtered match set.
        storage
            .index(doc("task", "t3", "dns fix", "dns resolver cache"))
            .await
            .unwrap();

        // None: whole-index aggregation.
        let all = storage.facet_counts(None).await.unwrap();
        assert_eq!(all["type"]["task"], 3);
        assert_eq!(all["type"]["mr"], 1);
        assert_eq!(all["workspace"]["ws-a"], 1);
        assert_eq!(all["status"]["in_progress"], 2);
        assert_eq!(all["status"]["done"], 1);
        assert_eq!(all["priority"]["high"], 1);

        // Some(query): the query's filtered match set. limit=1 must be IGNORED
        // — facet counts describe the full filtered set, not the result page.
        let filtered = storage
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
        assert_eq!(filtered["priority"]["high"], 1);

        // entity_type filter restricts the aggregation set.
        let mr_only = storage
            .facet_counts(Some(
                SearchQuery::parse("jwt".to_string())
                    .with_entity_type(Some("mr".to_string()))
                    .with_workspace_id(None)
                    .with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(mr_only["type"]["mr"], 1);
        assert!(
            !mr_only["type"].contains_key("task"),
            "entity_type filter must exclude tasks: {mr_only:?}"
        );

        // workspace_id filter restricts the aggregation set.
        let ws_only = storage
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

        // No matching term at all: empty aggregation, not whole-index.
        let none = storage
            .facet_counts(Some(
                SearchQuery::parse("zzz-nonexistent".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            ))
            .await
            .unwrap();
        assert!(
            none.is_empty(),
            "non-matching query must aggregate nothing: {none:?}"
        );
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
            .search(
                SearchQuery::parse("obsolete".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert!(stale.is_empty(), "upsert left a stale row");

        let fresh = storage
            .search(
                SearchQuery::parse("fresh".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].title, "new title");

        storage.delete("task", "t1").await.unwrap();
        let gone = storage
            .search(
                SearchQuery::parse("fresh".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert!(gone.is_empty());
    }

    #[tokio::test]
    async fn reindex_all_clears_and_reports_count() {
        let (_f, storage) = tmp_storage();
        storage
            .index(doc("task", "t1", "a", "termone text"))
            .await
            .unwrap();
        storage
            .index(doc("task", "t2", "b", "termone text"))
            .await
            .unwrap();
        assert_eq!(storage.reindex_all().await.unwrap(), 2);
        let after = storage
            .search(
                SearchQuery::parse("termone".to_string())
                    .with_entity_type(None)
                    .with_workspace_id(None)
                    .with_limit(10),
            )
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
            .search(
                SearchQuery::parse("orchestrator".to_string())
                    .with_entity_type(Some("agent".to_string()))
                    .with_workspace_id(None)
                    .with_limit(10),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "a1");
    }

    // ── §Query Language (task-154) ───────────────────────────────────────────

    /// Helper: search ids for a raw query-language string.
    async fn q(storage: &SqliteStorage, query: &str) -> Vec<String> {
        storage
            .search(SearchQuery::parse(query.to_string()).with_limit(10))
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.entity_id)
            .collect()
    }

    #[tokio::test]
    async fn quoted_phrases_match_exactly_not_individual_terms() {
        let (_f, storage) = tmp_storage();
        // "merge queue" adjacent.
        storage
            .index(doc(
                "task",
                "adjacent",
                "Queue rules",
                "the merge queue ordering rules",
            ))
            .await
            .unwrap();
        // Both words present, never adjacent — a phrase query must not match.
        storage
            .index(doc(
                "task",
                "scattered",
                "Merge separately",
                "merge the queue then ordering",
            ))
            .await
            .unwrap();

        // Phrase + term (spec example shape: `"merge queue" ordering`).
        let ids = q(&storage, "\"merge queue\" ordering").await;
        assert_eq!(ids, ["adjacent"], "phrase must require adjacency: {ids:?}");

        // Unquoted, the same words are independent AND terms — both match.
        let ids = q(&storage, "merge queue").await;
        assert_eq!(
            ids.len(),
            2,
            "unquoted terms must match independently: {ids:?}"
        );
    }

    #[tokio::test]
    async fn facet_filters_and_or_within_compose() {
        let (_f, storage) = tmp_storage();
        let mut t1 = doc("task", "t1", "jwt one", "jwt rotation one");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        t1.facets.insert("priority".to_string(), "high".to_string());
        let mut t2 = doc("task", "t2", "jwt two", "jwt rotation two");
        t2.facets.insert("status".to_string(), "done".to_string());
        t2.facets.insert("priority".to_string(), "high".to_string());
        let mut m1 = doc("mr", "m1", "jwt merge", "jwt handling");
        m1.facets.insert("status".to_string(), "open".to_string());
        m1.repo_id = Some("gyre-server".to_string());
        let mut t3 = doc("task", "t3", "jwt three", "jwt rotation three");
        t3.workspace_id = Some("ws-a".to_string());
        for d in [t1, t2, m1, t3] {
            storage.index(d).await.unwrap();
        }

        // type: narrows to tasks (entity_type column filter).
        let mut ids = q(&storage, "jwt type:task").await;
        ids.sort();
        assert_eq!(ids, ["t1", "t2", "t3"]);
        // OR within one facet: multiple values for the same key.
        let mut ids = q(&storage, "jwt type:task type:mr").await;
        ids.sort();
        assert_eq!(ids, ["m1", "t1", "t2", "t3"]);
        // AND across facets: type + metadata-facet (status) filter.
        assert_eq!(
            q(&storage, "jwt type:task status:in_progress").await,
            ["t1"]
        );
        // Metadata facet alone.
        let mut ids = q(&storage, "jwt priority:high").await;
        ids.sort();
        assert_eq!(ids, ["t1", "t2"]);
        // Workspace facet (column filter).
        assert_eq!(q(&storage, "jwt workspace:ws-a").await, ["t3"]);
        // Repo facet (column filter).
        assert_eq!(q(&storage, "jwt repo:gyre-server").await, ["m1"]);
        // Facet value that matches nothing: no results, not everything.
        assert!(q(&storage, "jwt status:approved").await.is_empty());
    }

    #[tokio::test]
    async fn time_facets_since_and_until_relative_and_absolute() {
        let (_f, storage) = tmp_storage();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let mut old = doc("task", "ancient", "Old doc", "ancient history keyword");
        old.updated_at = Some(now - 40 * 86_400);
        let mut recent = doc("task", "fresh", "New doc", "fresh news keyword");
        recent.updated_at = Some(now);
        storage.index(old).await.unwrap();
        storage.index(recent).await.unwrap();

        // Relative since: 7d excludes the 40-day-old doc.
        assert_eq!(q(&storage, "keyword since:7d").await, ["fresh"]);
        assert_eq!(q(&storage, "keyword since:2w").await, ["fresh"], "2w = 14d");
        // Absolute since far in the past keeps both.
        assert_eq!(q(&storage, "keyword since:2000-01-01").await.len(), 2);
        // Absolute since in the future keeps neither.
        assert!(q(&storage, "keyword since:2999-01-01").await.is_empty());
        // Relative until: "until 7 days ago" excludes the fresh doc, keeps old.
        assert_eq!(q(&storage, "keyword until:7d").await, ["ancient"]);
        // Absolute until covering today keeps both (end of that UTC day).
        assert_eq!(q(&storage, "keyword until:2999-01-01").await.len(), 2);
        // Absolute until in the past keeps neither.
        assert!(q(&storage, "keyword until:2000-01-01").await.is_empty());
        // Compose with a facet too.
        assert_eq!(q(&storage, "keyword type:task since:7d").await, ["fresh"]);
    }

    #[tokio::test]
    async fn facet_only_query_browses_without_text() {
        let (_f, storage) = tmp_storage();
        let mut t1 = doc("task", "t1", "Widget plan", "widget roadmap");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        let mut m1 = doc("mr", "m1", "Widget merge", "widget refactor");
        m1.facets.insert("status".to_string(), "open".to_string());
        storage.index(t1).await.unwrap();
        storage.index(m1).await.unwrap();

        // No text at all — the facet filter alone defines the match set.
        let ids = q(&storage, "status:open").await;
        assert_eq!(ids, ["m1"], "facet-only query must browse: {ids:?}");
        let ids = q(&storage, "type:mr status:open").await;
        assert_eq!(ids, ["m1"]);

        // A query that parses to nothing (malformed facet only) matches nothing.
        assert!(q(&storage, "type:").await.is_empty());
    }

    #[tokio::test]
    async fn facet_counts_honor_query_language_filters() {
        let (_f, storage) = tmp_storage();
        let mut t1 = doc("task", "t1", "jwt one", "jwt rotation one");
        t1.facets
            .insert("status".to_string(), "in_progress".to_string());
        let mut t2 = doc("task", "t2", "jwt two", "jwt rotation two");
        t2.facets.insert("status".to_string(), "done".to_string());
        storage.index(t1).await.unwrap();
        storage.index(t2).await.unwrap();

        // Text + facet filter: aggregation over the filtered match set only.
        let counts = storage
            .facet_counts(Some(
                SearchQuery::parse("jwt status:in_progress".to_string()).with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(counts["type"]["task"], 1);
        assert_eq!(counts["status"]["in_progress"], 1);
        assert!(
            !counts["status"].contains_key("done"),
            "filtered-out doc counted: {counts:?}"
        );

        // Facet-only query: aggregation set is the facet's browse set.
        let counts = storage
            .facet_counts(Some(
                SearchQuery::parse("status:done".to_string()).with_limit(10),
            ))
            .await
            .unwrap();
        assert_eq!(counts["status"]["done"], 1);
        assert_eq!(counts["type"]["task"], 1);

        // The reserved timestamp key is storage, never an aggregated facet.
        let all = storage.facet_counts(None).await.unwrap();
        assert!(
            !all.contains_key("updated_at"),
            "timestamp key leaked into facets: {all:?}"
        );
    }
}
