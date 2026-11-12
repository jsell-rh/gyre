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

/// §Access Scoping WHERE clauses for a query over `search_index`: the
/// caller's resolved scope (tenant boundary + accessible workspace/repo
/// sets), pushed into the same WHERE builder as the user filters so every
/// restriction is evaluated by the index query itself — never
/// post-filtered — keeping result, suggestion, and facet counts from
/// leaking inaccessible documents (search.md §Access Scoping). Every
/// clause references the `s` alias, so the base SQL in
/// `search()`/`suggest()`/`facet_counts()` MUST alias `search_index` AS
/// `s` (an unaliased FROM makes PG reject the query with 42P01). An empty
/// accessible set compiles to `FALSE` (zero rows, by query).
fn build_scope(query: &SearchQuery, w: &mut PgWhereBuilder) {
    // Tenant boundary — every result must belong to this tenant.
    let t = w.text(query.tenant_id.clone());
    w.push(format!("s.tenant_id = {t}"));
    // Accessible-workspace set: None = unscoped within the tenant (Admin);
    // an empty set excludes everything.
    match &query.accessible_workspace_ids {
        None => {}
        Some(ids) if ids.is_empty() => w.push("FALSE".to_string()),
        Some(ids) => {
            let placeholders: Vec<String> = ids.iter().map(|v| w.text(v.clone())).collect();
            w.push(format!("s.workspace_id IN ({})", placeholders.join(", ")));
        }
    }
    // Accessible-repo set: repo-less (workspace-level) documents stay
    // visible — repo scope restricts repos, not the workspace itself.
    match &query.accessible_repo_ids {
        None => {}
        Some(ids) if ids.is_empty() => w.push("FALSE".to_string()),
        Some(ids) => {
            let placeholders: Vec<String> = ids.iter().map(|v| w.text(v.clone())).collect();
            w.push(format!(
                "(s.repo_id = '' OR s.repo_id IN ({}))",
                placeholders.join(", ")
            ));
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
    // The tsquery join alone filters nothing (it would be a cross join that
    // only feeds ranking); the full-text predicate `s.tsv @@ q` is the actual
    // row filter for terms and quoted phrases (§Simple Search / §Quoted
    // Phrases). It must be emitted here so both search() and facet_counts()
    // restrict to the text-matched rows.
    if has_text {
        w.push("s.tsv @@ q".to_string());
    }
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
                let placeholders: Vec<String> = values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.entity_type IN ({})", placeholders.join(", ")));
            }
            FacetFilter::Workspace(values) => {
                let placeholders: Vec<String> = values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.workspace_id IN ({})", placeholders.join(", ")));
            }
            FacetFilter::Repo(values) => {
                let placeholders: Vec<String> = values.iter().map(|v| w.text(v.clone())).collect();
                w.push(format!("s.repo_id IN ({})", placeholders.join(", ")));
            }
            // Metadata facets (status/priority/label/…) live in the JSONB
            // column; the key is a bound parameter and the key set is
            // whitelisted by the parser, so no user text is interpolated.
            FacetFilter::Metadata { key, values } => {
                let key_p = w.text(key);
                let placeholders: Vec<String> = values.iter().map(|v| w.text(v.clone())).collect();
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

            // Tenant scope (§Access Scoping): caller-provided when known,
            // otherwise derived from the workspace's real scope row. Empty
            // string when the workspace is unknown — never a fabricated scope.
            let tenant_id = match doc.tenant_id.as_deref() {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => match doc.workspace_id.as_deref() {
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
                },
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
            // §Access Scoping: the tenant boundary and accessible
            // workspace/repo sets are WHERE clauses in this same index
            // query — never post-filtered — so result counts cannot leak
            // inaccessible documents.
            build_scope(&query, &mut w);

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
            .await??,
        )
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

    async fn suggest(&self, query: SearchQuery) -> Result<Vec<SearchSuggestion>> {
        // Trimmed-empty prefixes match every title — treat them as "no
        // suggestion" (the handler guards this too; the port is safe alone).
        let prefix = query.query.trim().to_string();
        if prefix.is_empty() {
            return Ok(vec![]);
        }
        let pattern = format!("{}%", escape_like(&prefix));
        let limit = query.limit;
        let pool = Arc::clone(&self.pool);
        Ok(
            tokio::task::spawn_blocking(move || -> Result<Vec<SearchSuggestion>> {
                let mut conn = pool.get().context("get db connection")?;
                // ILIKE is case-insensitive prefix matching on the title column —
                // what autocomplete needs (mid-word terms are unreachable from a
                // prefix).
                //
                // §Access Scoping: the same scope WHERE clauses as `search()`
                // are part of this index query — suggestions never include
                // titles the caller cannot access. The entity_type/
                // workspace_id shortcuts narrow within that scope. The FROM
                // aliases `search_index` AS `s` because the scope clauses
                // reference `s.*`.
                let mut w = PgWhereBuilder::new();
                let p = w.text(pattern);
                w.push(format!("s.title ILIKE {p} ESCAPE '\\'"));
                if let Some(et) = &query.entity_type {
                    let p = w.text(et.clone());
                    w.push(format!("s.entity_type = {p}"));
                }
                if let Some(ws) = &query.workspace_id {
                    // User-supplied narrowing — intersects with the accessible
                    // set, so it can only narrow, never widen.
                    let p = w.text(ws.clone());
                    w.push(format!("s.workspace_id = {p}"));
                }
                build_scope(&query, &mut w);
                let limit = w.bigint(limit as i64);
                let sql = format!(
                    "SELECT s.entity_type, s.entity_id, s.title
                 FROM search_index s{} ORDER BY s.title LIMIT {limit}",
                    w.sql()
                );
                let mut stmt = diesel::sql_query(sql).into_boxed::<diesel::pg::Pg>();
                for param in w.params {
                    stmt = match param {
                        PgParam::Text(v) => stmt.bind::<Text, _>(v),
                        PgParam::BigInt(v) => stmt.bind::<BigInt, _>(v),
                    };
                }
                let rows = stmt.load::<SuggestRow>(&mut conn)?;

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

    async fn facet_counts(&self, query: SearchQuery) -> Result<FacetCounts> {
        let pool = Arc::clone(&self.pool);
        Ok(tokio::task::spawn_blocking(move || -> Result<FacetCounts> {
            let mut conn = pool.get().context("get db connection")?;

            // Both shapes are §Access-scoped: a blank query (no text, no
            // user filters) aggregates the caller's whole accessible set
            // (the /search/facets endpoint); any text, facet, or shortcut
            // filter restricts the aggregation to the same filtered match
            // set `search()` uses — without the result limit, since counts
            // must describe the full filtered set, not the truncated page,
            // and must never count documents the caller cannot access.
            let mut w = PgWhereBuilder::new();
            let (_, from_tsq) = build_filters(&query, &mut w);
            build_scope(&query, &mut w);
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
            let rows = stmt.load::<FacetRow>(&mut conn)?;

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

#[cfg(test)]
mod tests {
    //! SQL-shape regression tests for the filter builder. The repo has no PG
    //! server harness, so these capture the generated statement shape — the
    //! exact surface where the task-154 review found the full-text predicate
    //! (`s.tsv @@ q`) had been dropped, leaving a ranking-only cross join,
    //! and where the access-scoping review guards the `s.*` alias and
    //! placeholder contiguity a live PostgreSQL enforces at parse time.

    use super::*;

    /// A text query MUST emit the `s.tsv @@ q` row filter — the tsquery FROM
    /// fragment alone filters nothing (cross join feeding ts_rank only), so
    /// without this predicate every text query returns all facet-passing
    /// rows and facet_counts(Some(q)) aggregates the whole index.
    #[test]
    fn text_query_emits_tsv_match_predicate() {
        let q = SearchQuery::parse("\"merge queue\" ordering");
        let mut w = PgWhereBuilder::new();
        let (has_text, from_tsq) = build_filters(&q, &mut w);
        assert!(has_text);
        assert!(from_tsq.contains("websearch_to_tsquery"));
        let sql = w.sql();
        assert!(
            sql.contains("s.tsv @@ q"),
            "full-text predicate missing from WHERE: {sql}"
        );
        // The WHERE clause is real (not empty), i.e. the predicate lands in
        // the statement's filter position, not just the builder's internals.
        assert!(sql.starts_with(" WHERE "));
    }

    /// Terms-only queries go through the same predicate (§Simple Search is
    /// AND-of-terms enforced by websearch_to_tsquery + the tsv match).
    #[test]
    fn simple_terms_query_emits_tsv_match_predicate() {
        let q = SearchQuery::parse("identity security JWT");
        let mut w = PgWhereBuilder::new();
        let (has_text, _) = build_filters(&q, &mut w);
        assert!(has_text);
        assert!(w.sql().contains("s.tsv @@ q"));
    }

    /// A facet-only query (§Faceted Filtering browse) intentionally has no
    /// text match: no FROM fragment, no tsv predicate, but real WHERE
    /// clauses for the facets.
    #[test]
    fn facet_only_query_has_no_tsv_predicate_but_real_filters() {
        let q = SearchQuery::parse("type:spec status:approved");
        let mut w = PgWhereBuilder::new();
        let (has_text, from_tsq) = build_filters(&q, &mut w);
        assert!(!has_text);
        assert!(from_tsq.is_empty());
        let sql = w.sql();
        assert!(!sql.contains("@@"));
        assert!(sql.contains("s.entity_type IN"));
        assert!(sql.contains("s.metadata->>"));
    }

    /// The tsquery parameter is reserved FIRST (referenced in the FROM
    /// clause, ahead of the WHERE placeholders), so the bind order emitted
    /// by `search()`/`facet_counts()` matches the placeholder numbering.
    #[test]
    fn tsquery_parameter_is_bound_first() {
        let q = SearchQuery::parse("\"merge queue\" type:spec workspace:platform-team");
        let mut w = PgWhereBuilder::new();
        let (_, from_tsq) = build_filters(&q, &mut w);
        // FROM references $1; every WHERE placeholder must number after it.
        assert!(from_tsq.contains("$1::text"), "from: {from_tsq}");
        let sql = w.sql();
        assert!(
            !sql.contains("$1::"),
            "WHERE reuses the FROM parameter: {sql}"
        );
        assert!(sql.contains("$2::text"));
    }

    // ── §Access Scoping (SQL-shape) ──────────────────────────────────────

    /// Caller-scoped query shaped like a real handler call: parse-built
    /// (raw query and its query-language parse stay consistent) with a
    /// resolved access scope attached.
    fn scoped_query() -> SearchQuery {
        SearchQuery::parse("deploy".to_string())
            .with_entity_type(Some("task".to_string()))
            .with_workspace_id(Some("ws-a".to_string()))
            .with_limit(10)
            .with_scope(
                "tenant-1".to_string(),
                Some(vec!["ws-a".to_string(), "ws-b".to_string()]),
                Some(vec!["repo-1".to_string()]),
            )
    }

    /// Every `$N` placeholder referenced by a SQL string, in order of
    /// appearance. ASCII digits after `$` only, so `$12` is one placeholder.
    fn placeholders(sql: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut chars = sql.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c != '$' {
                continue;
            }
            let mut end = i + 1;
            while let Some(&(j, cj)) = chars.peek() {
                if cj.is_ascii_digit() {
                    end = j + 1;
                    chars.next();
                } else {
                    break;
                }
            }
            if end > i + 1 {
                if let Ok(n) = sql[i + 1..end].parse::<usize>() {
                    out.push(n);
                }
            }
        }
        out
    }

    /// §Access Scoping invariants a live PostgreSQL enforces at parse time:
    ///
    /// - the scope clauses reference `s.*` columns, so the SQL heads in
    ///   `search()`/`suggest()`/`facet_counts()` MUST alias `search_index`
    ///   AS `s` — an unaliased FROM makes PG reject the whole query with
    ///   42P01 "missing FROM-clause entry for table \"s\"" (the regression
    ///   this guards: every suggest call 500ing on PG);
    /// - the tenant boundary is a WHERE clause of the index query itself,
    ///   never post-filtered;
    /// - placeholder numbering is contiguous, so every referenced
    ///   placeholder is one the caller actually binds.
    #[test]
    fn scope_clauses_are_s_prefixed_index_query_filters() {
        let mut w = PgWhereBuilder::new();
        build_filters(&scoped_query(), &mut w);
        build_scope(&scoped_query(), &mut w);
        let sql = w.sql();
        assert!(
            sql.contains("s.tenant_id = $"),
            "tenant boundary must be a WHERE clause of the index query: {sql}"
        );
        assert!(
            sql.contains("s.workspace_id IN ("),
            "accessible workspaces must restrict inside the query: {sql}"
        );
        assert!(
            sql.contains("(s.repo_id = '' OR s.repo_id IN ("),
            "repo scope must keep repo-less docs inside the query: {sql}"
        );
        // Contiguous numbering: the tsquery parameter is reserved first
        // ($1, referenced from the FROM clause), every WHERE placeholder
        // follows without gaps or reuse.
        let n = w.n;
        assert_eq!(
            placeholders(&sql),
            (2..=n).collect::<Vec<_>>(),
            "placeholder numbering must be contiguous 2..={n} in: {sql}"
        );
    }

    #[test]
    fn empty_accessible_set_compiles_to_no_rows() {
        let mut q = scoped_query();
        q.accessible_workspace_ids = Some(vec![]);
        let mut w = PgWhereBuilder::new();
        build_scope(&q, &mut w);
        assert!(
            w.sql().contains("FALSE"),
            "empty accessible set must yield zero rows by query, got: {}",
            w.sql()
        );
    }
}
