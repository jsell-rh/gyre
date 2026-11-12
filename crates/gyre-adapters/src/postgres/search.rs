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
use diesel::sql_types::{BigInt, Double, Nullable, Text};
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
    -- Scope is part of the identity: a spec/file path is only unique within
    -- its repo, so same-path documents in different repos must coexist.
    PRIMARY KEY (entity_type, entity_id, workspace_id, repo_id)
);
CREATE INDEX IF NOT EXISTS idx_search_index_tsv ON search_index USING gin (tsv);
";

/// Create the tsvector search table + GIN index on the PG-only init path,
/// then reconcile the primary key with the current schema (see
/// [`pk_upgrade_sql`]).
pub(crate) fn ensure_search_table(conn: &mut PgConnection) -> Result<()> {
    use diesel::connection::SimpleConnection;
    conn.batch_execute(TSVECTOR_DDL)
        .context("create tsvector search_index table")?;
    upgrade_search_index_pk(conn)?;
    Ok(())
}

/// task-202 widened the search_index identity from (entity_type, entity_id)
/// to (entity_type, entity_id, workspace_id, repo_id) and changed the upsert
/// conflict target to match. `CREATE TABLE IF NOT EXISTS` does not touch an
/// existing table's constraints, so a deployment initialized under task-201
/// keeps the 2-column PK — and every `INSERT ... ON CONFLICT (4 cols)` then
/// fails with SQLSTATE 42P10 ("no unique or exclusion constraint matching
/// the ON CONFLICT specification"). Indexing is best-effort, so the failure
/// mode is silent: writes succeed, the index stays empty. Reconcile here,
/// on the init path, before the new conflict target is ever used.
fn upgrade_search_index_pk(conn: &mut PgConnection) -> Result<()> {
    #[derive(QueryableByName)]
    struct PkRow {
        #[diesel(sql_type = Text)]
        constraint_name: String,
        #[diesel(sql_type = Text)]
        column_name: String,
    }

    let rows = diesel::sql_query(
        "SELECT tc.constraint_name, kcu.column_name
         FROM information_schema.table_constraints tc
         JOIN information_schema.key_column_usage kcu
           ON kcu.constraint_name = tc.constraint_name
          AND kcu.constraint_schema = tc.constraint_schema
         WHERE tc.table_schema = current_schema()
           AND tc.table_name = 'search_index'
           AND tc.constraint_type = 'PRIMARY KEY'
         ORDER BY kcu.ordinal_position",
    )
    .load::<PkRow>(conn)
    .context("read search_index primary key")?;

    let constraint_name = rows.first().map(|r| r.constraint_name.clone());
    let pk_cols: Vec<String> = rows.into_iter().map(|r| r.column_name).collect();
    if let Some(sql) = pk_upgrade_sql(&constraint_name, &pk_cols) {
        diesel::sql_query(&sql)
            .execute(conn)
            .with_context(|| format!("upgrade search_index primary key: {sql}"))?;
    }
    Ok(())
}

/// The primary key search_index must have for the current upsert conflict
/// target (`ON CONFLICT (entity_type, entity_id, workspace_id, repo_id)`).
const EXPECTED_SEARCH_INDEX_PK: [&str; 4] = ["entity_type", "entity_id", "workspace_id", "repo_id"];

/// Decide whether the live primary key needs reconciling, returning the
/// `ALTER TABLE` statement to run (or `None` when it already matches).
/// Pure — unit-testable without a live PostgreSQL instance.
///
/// * No PK at all → add the expected one.
/// * Old task-201 2-column PK → drop and re-add with scope columns. The old
///   key is a subset of the new one, so the wider key is already unique; the
///   scope columns are `NOT NULL` since task-201, so the ADD always applies.
/// * Anything else (including a wider/custom key that does not match) → also
///   reconciled to the canonical key, so the conflict target is guaranteed to
///   find its constraint.
fn pk_upgrade_sql(constraint_name: &Option<String>, pk_cols: &[String]) -> Option<String> {
    if pk_cols == EXPECTED_SEARCH_INDEX_PK {
        return None;
    }
    let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
    let add = "ADD PRIMARY KEY (entity_type, entity_id, workspace_id, repo_id)";
    match constraint_name {
        Some(name) if !pk_cols.is_empty() => Some(format!(
            "ALTER TABLE search_index DROP CONSTRAINT {}, {add}",
            quoted(name)
        )),
        _ => Some(format!("ALTER TABLE search_index {add}")),
    }
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
INSERT INTO search_index (entity_type, entity_id, tenant_id, workspace_id, repo_id, title, body, metadata)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
ON CONFLICT (entity_type, entity_id, workspace_id, repo_id) DO UPDATE SET
    tenant_id  = EXCLUDED.tenant_id,
    workspace_id = EXCLUDED.workspace_id,
    repo_id    = EXCLUDED.repo_id,
    title      = EXCLUDED.title,
    body       = EXCLUDED.body,
    metadata   = EXCLUDED.metadata
";

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
            if query.query.trim().is_empty() {
                return Ok(vec![]);
            }
            // websearch_to_tsquery applies implicit AND between terms (spec
            // §Simple Search default) and tolerates punctuation without a
            // syntax error. entity_type/workspace_id filters are WHERE clauses
            // evaluated by the index query itself, not post-filtered.
            // ts_headline wraps matches in ** markers, mirroring the FTS5 adapter.
            let rows = diesel::sql_query(
                "SELECT s.entity_type,
                        s.entity_id,
                        s.title,
                        ts_rank(s.tsv, q)::float8 AS rank,
                        ts_headline('english', s.body, q,
                                    'StartSel=**,StopSel=**,MaxFragments=1,MinWords=5,MaxWords=12') AS snip,
                        s.metadata::text AS metadata
                 FROM search_index s, websearch_to_tsquery('english', $1) q
                 WHERE s.tsv @@ q
                   AND ($2::text IS NULL OR s.entity_type = $2)
                   AND ($3::text IS NULL OR s.workspace_id = $3)
                 ORDER BY rank DESC
                 LIMIT $4",
            )
            .bind::<Text, _>(&query.query)
            .bind::<Nullable<Text>, _>(query.entity_type)
            .bind::<Nullable<Text>, _>(query.workspace_id)
            .bind::<BigInt, _>(query.limit as i64)
            .load::<TsRow>(&mut conn)?;

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
            // filtered match set `search()` uses, without the result limit —
            // facet counts must describe the full filtered set, not the
            // truncated page.
            let rows: Vec<FacetRow> = match query {
                None => diesel::sql_query(
                    "SELECT entity_type, workspace_id, metadata::text FROM search_index",
                )
                .load::<FacetRow>(&mut conn)?,
                Some(q) => {
                    if q.query.trim().is_empty() {
                        return Ok(FacetCounts::new());
                    }
                    diesel::sql_query(
                        "SELECT s.entity_type, s.workspace_id, s.metadata::text
                         FROM search_index s, websearch_to_tsquery('english', $1) q
                         WHERE s.tsv @@ q
                           AND ($2::text IS NULL OR s.entity_type = $2)
                           AND ($3::text IS NULL OR s.workspace_id = $3)",
                    )
                    .bind::<Text, _>(&q.query)
                    .bind::<Nullable<Text>, _>(q.entity_type)
                    .bind::<Nullable<Text>, _>(q.workspace_id)
                    .load::<FacetRow>(&mut conn)?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pk_upgrade_is_noop_when_pk_already_matches() {
        let cols: Vec<String> = EXPECTED_SEARCH_INDEX_PK
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            pk_upgrade_sql(&Some("search_index_pkey".into()), &cols),
            None
        );
    }

    #[test]
    fn pk_upgrade_drops_task201_two_column_pk() {
        // A deployment initialized under task-201 keeps the 2-column PK;
        // CREATE TABLE IF NOT EXISTS never reconciles it.
        let cols: Vec<String> = ["entity_type", "entity_id"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            pk_upgrade_sql(&Some("search_index_pkey".into()), &cols),
            Some(
                "ALTER TABLE search_index DROP CONSTRAINT \"search_index_pkey\", \
                 ADD PRIMARY KEY (entity_type, entity_id, workspace_id, repo_id)"
                    .to_string()
            )
        );
    }

    #[test]
    fn pk_upgrade_quotes_hostile_constraint_names() {
        let cols: Vec<String> = ["entity_type", "entity_id"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let sql = pk_upgrade_sql(&Some("bad\"name".into()), &cols).unwrap();
        assert!(
            sql.contains("DROP CONSTRAINT \"bad\"\"name\""),
            "got: {sql}"
        );
    }

    #[test]
    fn pk_upgrade_adds_when_no_pk_exists() {
        assert_eq!(
            pk_upgrade_sql(&None, &[]),
            Some(
                "ALTER TABLE search_index \
                 ADD PRIMARY KEY (entity_type, entity_id, workspace_id, repo_id)"
                    .to_string()
            )
        );
    }

    #[test]
    fn pk_upgrade_reconciles_any_nonmatching_key() {
        let cols: Vec<String> = ["entity_id", "entity_type"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(pk_upgrade_sql(&Some("custom_pk".into()), &cols).is_some());
    }
}
