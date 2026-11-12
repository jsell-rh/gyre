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
use diesel::sql_types::{Array, BigInt, Double, Text};
use gyre_ports::search::{
    FacetCounts, SearchDocument, SearchPort, SearchQuery, SearchResult, SearchSuggestion,
};
use std::{collections::HashMap, sync::Arc};

use super::PgStorage;

/// PG-only schema: scope columns + JSONB facets + generated tsvector + GIN index.
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
    tsv tsvector GENERATED ALWAYS AS (to_tsvector('english', title || ' ' || body)) STORED,
    PRIMARY KEY (entity_type, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_search_index_tsv ON search_index USING gin (tsv);
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

/// A typed bind emitted by [`scope_filter_sql`].
enum ScopeBind {
    Text(String),
    TextArray(Vec<String>),
}

/// §Access Scoping WHERE fragments for a query over `search_index`: the
/// user-supplied `entity_type`/`workspace_id` narrowing filters plus the
/// caller's resolved scope (tenant boundary + accessible workspace/repo
/// sets), as `$N` placeholders numbered from `*next` (advanced past every
/// emitted bind on return).
///
/// Callers append this to their boxed raw query before the ORDER BY/LIMIT
/// tail so every restriction is evaluated by the index query itself — never
/// post-filtered — keeping result, suggestion, and facet counts from leaking
/// inaccessible documents (search.md §Access Scoping). An empty accessible
/// set compiles to `AND FALSE` (zero rows, by query).
fn scope_filter_sql(query: &SearchQuery, next: &mut usize) -> (String, Vec<ScopeBind>) {
    let mut sql = String::new();
    let mut binds = Vec::new();
    let mut n = *next;

    if let Some(et) = &query.entity_type {
        sql.push_str(&format!(" AND s.entity_type = ${n}"));
        binds.push(ScopeBind::Text(et.clone()));
        n += 1;
    }
    if let Some(ws) = &query.workspace_id {
        // User-supplied narrowing filter — intersects with the accessible
        // set below, so it can only narrow, never widen.
        sql.push_str(&format!(" AND s.workspace_id = ${n}"));
        binds.push(ScopeBind::Text(ws.clone()));
        n += 1;
    }
    match &query.accessible_workspace_ids {
        None => {}
        Some(ids) if ids.is_empty() => {
            // Caller has access to no workspace: no rows, by query.
            sql.push_str(" AND FALSE");
        }
        Some(ids) => {
            sql.push_str(&format!(" AND s.workspace_id = ANY(${n})"));
            binds.push(ScopeBind::TextArray(ids.clone()));
            n += 1;
        }
    }
    match &query.accessible_repo_ids {
        None => {}
        Some(ids) if ids.is_empty() => {
            sql.push_str(" AND FALSE");
        }
        Some(ids) => {
            // Repo-less (workspace-level) documents stay visible: repo scope
            // restricts repos, not the workspace itself.
            sql.push_str(&format!(" AND (s.repo_id = '' OR s.repo_id = ANY(${n}))"));
            binds.push(ScopeBind::TextArray(ids.clone()));
            n += 1;
        }
    }
    sql.push_str(&format!(" AND s.tenant_id = ${n}"));
    binds.push(ScopeBind::Text(query.tenant_id.clone()));
    n += 1;

    *next = n;
    (sql, binds)
}

const INSERT_SQL: &str = "
INSERT INTO search_index (entity_type, entity_id, tenant_id, workspace_id, repo_id, title, body, metadata)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    tenant_id  = EXCLUDED.tenant_id,
    workspace_id = EXCLUDED.workspace_id,
    repo_id    = EXCLUDED.repo_id,
    title      = EXCLUDED.title,
    body       = EXCLUDED.body,
    metadata   = EXCLUDED.metadata
";

/// Base SQL for `search()`: full-text match over the tsvector index. Aliases
/// `search_index` AS `s` because the §Access Scoping fragments appended by
/// [`scope_filter_sql`] reference `s.*`. $1 = websearch tsquery; scope
/// fragments continue from $2.
const SEARCH_BASE_SQL: &str = "
SELECT s.entity_type,
       s.entity_id,
       s.title,
       ts_rank(s.tsv, q)::float8 AS rank,
       ts_headline('english', s.body, q,
                   'StartSel=**,StopSel=**,MaxFragments=1,MinWords=5,MaxWords=12') AS snip,
       s.metadata::text AS metadata
FROM search_index s, websearch_to_tsquery('english', $1) q
WHERE s.tsv @@ q";

/// Base SQL for `suggest()`: prefix match on the title column. MUST alias
/// `search_index` AS `s` — the §Access Scoping fragments appended by
/// [`scope_filter_sql`] reference `s.*`, and an unaliased FROM makes
/// PostgreSQL reject the whole query with 42P01 "missing FROM-clause entry
/// for table \"s\"" (every suggest call would 500 on PG). $1 = LIKE
/// pattern; scope fragments continue from $2.
const SUGGEST_BASE_SQL: &str = "
SELECT s.entity_type, s.entity_id, s.title
FROM search_index s
WHERE s.title ILIKE $1 ESCAPE '\\'";

/// Base SQL for `facet_counts()` with a text query: aggregates the same
/// filtered match set `search()` uses. $1 = websearch tsquery.
const FACET_TEXT_BASE_SQL: &str = "
SELECT s.entity_type, s.workspace_id, s.metadata::text
FROM search_index s, websearch_to_tsquery('english', $1) q
WHERE s.tsv @@ q";

/// Base SQL for `facet_counts()` with blank text: aggregates the caller's
/// whole accessible set. `WHERE TRUE` anchors the scope fragments appended
/// below. Placeholders start at $1 (no tsquery bind).
const FACET_BLANK_BASE_SQL: &str = "
SELECT s.entity_type, s.workspace_id, s.metadata::text
FROM search_index s
WHERE TRUE";

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
                .execute(&mut *conn)?;
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
                if query.query.trim().is_empty() {
                    return Ok(vec![]);
                }
                // websearch_to_tsquery applies implicit AND between terms (spec
                // §Simple Search default) and tolerates punctuation without a
                // syntax error. entity_type/workspace_id filters and the
                // §Access Scoping constraints (tenant + accessible workspace/repo
                // sets) are WHERE clauses evaluated by the index query itself,
                // never post-filtered, so result counts cannot leak inaccessible
                // documents. ts_headline wraps matches in ** markers, mirroring
                // the FTS5 adapter.
                let mut q = diesel::sql_query(SEARCH_BASE_SQL).into_boxed();
                q = q.bind::<Text, _>(query.query.clone());
                let mut n = 2;

                let (scope_sql, scope_binds) = scope_filter_sql(&query, &mut n);
                q = q.sql(scope_sql);
                for bind in scope_binds {
                    q = match bind {
                        ScopeBind::Text(v) => q.bind::<Text, _>(v),
                        ScopeBind::TextArray(v) => q.bind::<Array<Text>, _>(v),
                    };
                }
                q = q
                    .sql(format!(" ORDER BY rank DESC LIMIT ${n}"))
                    .bind::<BigInt, _>(query.limit as i64);

                let rows = q.load::<TsRow>(&mut conn)?;

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
                // prefix). §Access Scoping: the same WHERE fragments as
                // `search()` are part of this index query — suggestions never
                // include titles the caller cannot access.
                let mut q = diesel::sql_query(SUGGEST_BASE_SQL).into_boxed();
                q = q.bind::<Text, _>(pattern);
                let mut n = 2;

                let (scope_sql, scope_binds) = scope_filter_sql(&query, &mut n);
                q = q.sql(scope_sql);
                for bind in scope_binds {
                    q = match bind {
                        ScopeBind::Text(v) => q.bind::<Text, _>(v),
                        ScopeBind::TextArray(v) => q.bind::<Array<Text>, _>(v),
                    };
                }
                q = q
                    .sql(format!(" ORDER BY s.title LIMIT ${n}"))
                    .bind::<BigInt, _>(limit as i64);
                let rows = q.load::<SuggestRow>(&mut conn)?;

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

            // Two shapes, both §Access-scoped: a blank text query aggregates
            // the caller's whole accessible set (the /search/facets endpoint);
            // a text query aggregates the same filtered match set `search()`
            // uses, without the result limit — facet counts must describe the
            // full filtered set, not the truncated page, and must never count
            // documents the caller cannot access.
            let has_text = !query.query.trim().is_empty();
            let base_sql = if has_text {
                FACET_TEXT_BASE_SQL
            } else {
                FACET_BLANK_BASE_SQL
            };
            let mut q = diesel::sql_query(base_sql).into_boxed();
            let mut n = 1;
            if has_text {
                q = q.bind::<Text, _>(query.query.clone());
                n = 2;
            }

            let (scope_sql, scope_binds) = scope_filter_sql(&query, &mut n);
            q = q.sql(scope_sql);
            for bind in scope_binds {
                q = match bind {
                    ScopeBind::Text(v) => q.bind::<Text, _>(v),
                    ScopeBind::TextArray(v) => q.bind::<Array<Text>, _>(v),
                };
            }
            let rows = q.load::<FacetRow>(&mut conn)?;

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
    use super::*;

    fn scoped_query() -> SearchQuery {
        SearchQuery {
            query: "deploy".to_string(),
            entity_type: Some("task".to_string()),
            workspace_id: Some("ws-a".to_string()),
            limit: 10,
            tenant_id: "tenant-1".to_string(),
            accessible_workspace_ids: Some(vec!["ws-a".to_string(), "ws-b".to_string()]),
            accessible_repo_ids: Some(vec!["repo-1".to_string()]),
        }
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

    /// Compose a base SQL with the §Access Scoping fragments exactly the way
    /// `search()`/`suggest()`/`facet_counts()` do, then assert the invariants
    /// a live PostgreSQL enforces at parse time:
    ///
    /// - the scope fragments reference `s.*` columns, so the base SQL MUST
    ///   alias `search_index` AS `s` — an unaliased FROM makes PG reject the
    ///   whole query with 42P01 "missing FROM-clause entry for table \"s\""
    ///   (the regression this guards: every suggest call 500ing on PG);
    /// - the tenant boundary is a WHERE clause of the index query itself,
    ///   never post-filtered;
    /// - placeholder numbering is contiguous 1..next, so every referenced
    ///   placeholder is one the caller actually binds.
    fn assert_scope_composable(base: &str, first_free: usize) {
        let mut n = first_free;
        let (frag, _binds) = scope_filter_sql(&scoped_query(), &mut n);
        let composed = format!("{base}{frag}");
        assert!(
            composed.contains("FROM search_index s"),
            "base SQL must alias search_index AS `s` (scope fragments reference s.*): {composed}"
        );
        assert!(
            composed.contains("AND s.tenant_id = $"),
            "tenant boundary must be a WHERE clause of the index query: {composed}"
        );
        assert_eq!(
            placeholders(&composed),
            (1..n).collect::<Vec<_>>(),
            "placeholder numbering must be contiguous 1..{n} in: {composed}"
        );
    }

    #[test]
    fn search_base_sql_defines_the_scope_alias() {
        assert_scope_composable(SEARCH_BASE_SQL, 2);
    }

    #[test]
    fn suggest_base_sql_defines_the_scope_alias() {
        assert_scope_composable(SUGGEST_BASE_SQL, 2);
    }

    #[test]
    fn facet_base_sqls_define_the_scope_alias() {
        assert_scope_composable(FACET_TEXT_BASE_SQL, 2);
        assert_scope_composable(FACET_BLANK_BASE_SQL, 1);
    }

    #[test]
    fn empty_accessible_set_compiles_to_no_rows() {
        let mut q = scoped_query();
        q.accessible_workspace_ids = Some(vec![]);
        let mut n = 2;
        let (frag, _) = scope_filter_sql(&q, &mut n);
        assert!(
            frag.contains(" AND FALSE"),
            "empty accessible set must yield zero rows by query, got: {frag}"
        );
    }
}
