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

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Double, Nullable, Text};
use gyre_ports::search::{SearchDocument, SearchPort, SearchQuery, SearchResult};
use std::sync::Arc;

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

#[derive(QueryableByName)]
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
VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
ON CONFLICT (entity_type, entity_id) DO UPDATE SET
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
                        ts_rank(s.tsv, q) AS rank,
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
