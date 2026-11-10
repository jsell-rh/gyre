//! Persistent full-text search adapter — PostgreSQL tsvector/tsquery.
//!
//! Spec: search.md §Search Index (Technology). Postgres deployments use the
//! built-in full-text engine: a `tsv tsvector` generated column with a GIN
//! index, `websearch_to_tsquery` matching (implicit AND-of-terms), `ts_rank`
//! relevance, `ts_headline` snippets with `**` match markers. Zero external
//! search infrastructure.
//!
//! The DDL is PG-dialect SQL and lives on the PostgreSQL-only init path
//! (`PgStorage::new_for_tenant`), NOT the shared diesel `migrations/` dir —
//! that dir is executed by the SQLite backend too.

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
    SearchSuggestion,
};
use std::{collections::HashMap, sync::Arc};

use super::PgStorage;

/// Current unix time in seconds (document timestamps for `since:`/`until:`).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// PG-only schema: scope columns + JSONB facets + generated tsvector + GIN index.
/// `updated_at` backs the `since:`/`until:` facets (§Query Language); the
/// ALTER covers tables created before the column existed (regular PG table,
/// so unlike FTS5 it can be extended in place).
pub(crate) const TSVECTOR_DDL: &str = "
CREATE TABLE IF NOT EXISTS search_index (
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    repo_id TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at BIGINT NOT NULL DEFAULT 0,
    tsv tsvector GENERATED ALWAYS AS (to_tsvector('english', title || ' ' || body)) STORED,
    PRIMARY KEY (entity_type, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_search_index_tsv ON search_index USING gin (tsv);
ALTER TABLE search_index ADD COLUMN IF NOT EXISTS updated_at BIGINT NOT NULL DEFAULT 0;
";

/// Create the tsvector search table + GIN index on the PG-only init path.
pub(crate) fn ensure_search_table(conn: &mut PgConnection) -> Result<()> {
    use diesel::connection::SimpleConnection;
    conn.batch_execute(TSVECTOR_DDL)
        .context("create tsvector search_index table")?;
    Ok(())
}

// `QueryableByName`'s derive expands to `Self { field: field, … }` with spans
// mapped onto these declarations, so clippy::redundant_field_names fires on
// generated code, not on anything written here.
#[derive(QueryableByName)]
#[allow(clippy::redundant_field_names)]
struct TsRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    entity_id: String,
    #[diesel(sql_type = Text)]
    title: String,
    /// ts_rank(...) — 0..1, higher = more relevant.
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
/// user-supplied prefix so it is matched literally under `ESCAPE '\\'`.
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
/// metadata facet key/value.
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
INSERT INTO search_index (entity_type, entity_id, tenant_id, workspace_id, repo_id, title, body, metadata, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)
ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    tenant_id  = EXCLUDED.tenant_id,
    workspace_id = EXCLUDED.workspace_id,
    repo_id    = EXCLUDED.repo_id,
    title      = EXCLUDED.title,
    body       = EXCLUDED.body,
    metadata   = EXCLUDED.metadata,
    updated_at = EXCLUDED.updated_at
";

/// Build the `websearch_to_tsquery` input for a parsed query (§Query
/// Language): terms joined by spaces plus phrases in double quotes.
/// websearch_to_tsquery AND-composes the pieces, treats `"…"` as an exact
/// phrase, and never raises a syntax error. The parser guarantees terms are
/// alphanumeric-only and phrases contain no `"`, so the reconstruction is
/// inert as syntax.
fn build_websearch_text(parsed: &gyre_ports::search::ParsedQuery) -> String {
    let mut parts: Vec<String> = parsed.terms.clone();
    parts.extend(parsed.phrases.iter().map(|p| format!("\"{p}\"")));
    parts.join(" ")
}

/// A dynamically bound SQL parameter — the WHERE clauses vary with the
/// parsed query's facet filters, so the statement shape is built per query.
enum PgParam {
    Text(String),
    BigInt(i64),
}

/// Incremental builder for the filter WHERE clause: emits numbered,
/// explicitly-cast placeholders (`$1::text`, `$2::bigint`, …) in bind order
/// and collects the values.
struct PgWhereBuilder {
    clauses: Vec<String>,
    params: Vec<PgParam>,
    n: usize,
}

impl PgWhereBuilder {
    fn new() -> Self {
        PgWhereBuilder {
            clauses: vec![],
            params: vec![],
            n: 0,
        }
    }

    fn text(&mut self, value: String) -> String {
        self.n += 1;
        self.params.push(PgParam::Text(value));
        format!("${}::text", self.n)
    }

    fn bigint(&mut self, value: i64) -> String {
        self.n += 1;
        self.params.push(PgParam::BigInt(value));
        format!("${}::bigint", self.n)
    }

    fn push(&mut self, clause: String) {
        self.clauses.push(clause);
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

/// Shared filter construction for `search()` and `facet_counts()`: full-text
/// match plus entity_type/workspace_id shortcuts plus facet filters plus
/// `since:`/`until:` bounds, AND-composed. When text is present its tsquery
/// parameter is reserved FIRST (it is referenced in the FROM clause, ahead of
/// the WHERE placeholders) and the FROM fragment is returned.
fn build_filters(query: &SearchQuery, w: &mut PgWhereBuilder) -> (bool, String) {
    let parsed = &query.parsed;
    let ws_text = build_websearch_text(parsed);
    let has_text = !ws_text.is_empty();
    let from_tsq = if has_text {
        let p = w.text(ws_text);
        format!(", websearch_to_tsquery('english', {p}) q")
    } else {
        String::new()
    };
    if let Some(et) = &query.entity_type {
        let p = w.text(et.clone());
        w.push(format!("s.entity_type = {p}"));
    }
    if let Some(ws) = &query.workspace_id {
        let p = w.text(ws.clone());
        w.push(format!("s.workspace_id = {p}"));
    }
    for filter in parsed.facet_filters() {
        match filter {
            FacetFilter::EntityType(values) => {
                let placeholders: Vec<String> =
                    values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.entity_type IN ({})", placeholders.join(", ")));
            }
            FacetFilter::Workspace(values) => {
                let placeholders: Vec<String> =
                    values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.workspace_id IN ({})", placeholders.join(", ")));
            }
            FacetFilter::Repo(values) => {
                let placeholders: Vec<String> =
                    values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.repo_id IN ({})", placeholders.join(", ")));
            }
            // Metadata facets (status/priority/label/…) live in the JSONB
            // column; the key is a bound parameter and the key set is
            // whitelisted by the parser, so no user text is interpolated.
            FacetFilter::Metadata { key, values } => {
                let key_p = w.text(key);
                let placeholders: Vec<String> =
                    values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!(
                    "s.metadata->>{key_p} IN ({})",
                    placeholders.join(", ")
                ));
            }
        }
    }
    let time = parsed.time_bounds(now_secs());
    if let Some(since) = time.since {
        let p = w.bigint(since);
        w.push(format!("s.updated_at >= {p}"));
    }
    if let Some(until) = time.until {
        let p = w.bigint(until);
        w.push(format!("s.updated_at <= {p}"));
    }
    (has_text, from_tsq)
}

#[async_trait]
impl SearchPort for PgStorage {
    async fn index(&self, doc: SearchDocument) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let metadata_json = serde_json::to_string(&doc.facets)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;

            // Derive tenant from the workspace's real scope row (§Access Scoping);
            // empty string when the workspace is unknown — never a fabricated scope.
            let tenant_id = match doc.workspace_id.as_deref() {
                Some(ws) if !ws.is_empty() => {
                    match diesel::sql_query("SELECT tenant_id FROM workspaces WHERE id = $1")
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

            diesel::sql_query(INSERT_SQL)
                .bind::<Text, _>(&doc.entity_type)
                .bind::<Text, _>(&doc.entity_id)
                .bind::<Text, _>(&tenant_id)
                .bind::<Text, _>(&workspace_id)
                .bind::<Text, _>(&repo_id)
                .bind::<Text, _>(&doc.title)
                .bind::<Text, _>(&doc.body)
                .bind::<Text, _>(&metadata_json)
                .bind::<BigInt, _>(doc.updated_at.unwrap_or_else(now_secs))
                .execute(&mut *conn)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        let pool = Arc::clone(&self.pool);
        Ok(tokio::task::spawn_blocking(move || -> Result<Vec<SearchResult>> {
            let mut conn = pool.get().context("get db connection")?;

            // §Query Language: full-text match (websearch_to_tsquery applies
            // implicit AND between terms and honors "quoted phrases") plus
            // facet filters, AND-composed; a facet-only query browses without
            // a text match. All filters are WHERE clauses evaluated by the
            // index query itself, not post-filtered.
            let mut w = PgWhereBuilder::new();
            let (has_text, from_tsq) = build_filters(&query, &mut w);
            if !has_text && w.is_empty() {
                return Ok(vec![]);
            }

            // ts_rank/ts_headline need the tsquery; without one (facet-only
            // browse) rank is a constant and the snippet is a body prefix.
            let (rank_snip, order) = if has_text {
                (
                    "ts_rank(s.tsv, q)::float8 AS rank,
                        ts_headline('english', s.body, q,
                                    'StartSel=**,StopSel=**,MaxFragments=1,MinWords=5,MaxWords=12') AS snip"
                        .to_string(),
                    "ORDER BY rank DESC".to_string(),
                )
            } else {
                (
                    "0.0::float8 AS rank,
                        substr(replace(s.body, chr(10), ' '), 1, 120) AS snip".to_string(),
                    "ORDER BY s.updated_at DESC".to_string(),
                )
            };
            let limit = w.bigint(query.limit as i64);
            let sql = format!(
                "SELECT s.entity_type,
                        s.entity_id,
                        s.title,
                        {rank_snip},
                        s.metadata::text AS metadata
                 FROM search_index s{from_tsq}{} {order} LIMIT {limit}",
                w.sql()
            );

            let mut stmt = diesel::sql_query(sql).into_boxed::<diesel::pg::Pg>();
            for param in w.params {
                stmt = match param {
                    PgParam::Text(v) => stmt.bind::<Text, _>(v),
                    PgParam::BigInt(v) => stmt.bind::<BigInt, _>(v),
                };
            }
            let rows = stmt.load::<TsRow>(&mut conn)?;

            Ok(rows
                .into_iter()
                .map(|r| {
                    // ts_rank is already a higher-is-better relevance in [0,1).
                    let facets = serde_json::from_str(&r.metadata).unwrap_or_default();
                    SearchResult {
                        entity_type: r.entity_type,
                        entity_id: r.entity_id,
                        title: r.title,
                        snippet: r.snip,
                        score: r.rank,
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
            diesel::sql_query("DELETE FROM search_index WHERE entity_type = $1 AND entity_id = $2")
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
                // ILIKE is case-insensitive prefix matching on the title column —
                // what autocomplete needs (mid-word terms are unreachable from a
                // prefix).
                let rows = diesel::sql_query(
                    "SELECT entity_type, entity_id, title
                 FROM search_index
                 WHERE title ILIKE $1 ESCAPE '\\'
                 ORDER BY title
                 LIMIT $2",
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
            // filtered match set `search()` uses (text + facets + shortcuts),
            // without the result limit — facet counts must describe the full
            // filtered set, not the truncated page.
            let rows: Vec<FacetRow> = match query {
                None => diesel::sql_query(
                    "SELECT entity_type, workspace_id, metadata::text FROM search_index",
                )
                .load::<FacetRow>(&mut conn)?,
                Some(q) => {
                    let mut w = PgWhereBuilder::new();
                    let (has_text, from_tsq) = build_filters(&q, &mut w);
                    if !has_text && w.is_empty() {
                        return Ok(FacetCounts::new());
                    }
                    let sql = format!(
                        "SELECT s.entity_type, s.workspace_id, s.metadata::text
                         FROM search_index s{from_tsq}{}",
                        w.sql()
                    );
                    let mut stmt = diesel::sql_query(sql).into_boxed::<diesel::pg::Pg>();
                    for param in w.params {
                        stmt = match param {
                            PgParam::Text(v) => stmt.bind::<Text, _>(v),
                            PgParam::BigInt(v) => stmt.bind::<BigInt, _>(v),
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
            diesel::sql_query("DELETE FROM search_index").execute(&mut *conn)?;
            Ok(count as u64)
        })
        .await??)
    }
}
