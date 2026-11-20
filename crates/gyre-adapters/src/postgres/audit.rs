use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_ports::{AuditQueryFilter, AuditRepository, CompressionStats};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::audit_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub(crate) struct AuditEventRow {
    id: String,
    event_type: String,
    agent_id: Option<String>,
    user_id: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    tenant_id: Option<String>,
    repo_id: Option<String>,
    resource_type: String,
    resource_id: Option<String>,
    outcome: String,
    detail: String,
    /// zstd-compressed detail JSON; NULL = row not yet compressed
    /// (log-compression job, observability.md §Log Retention &
    /// Compression).
    detail_zstd: Option<Vec<u8>>,
    source_ip: Option<String>,
    user_agent: Option<String>,
    timestamp: i64,
}

impl AuditEventRow {
    /// Materialize a domain event, decompressing the detail payload when
    /// the row is compressed. Decompression or JSON parse failure of a
    /// compressed payload is a hard error: a corrupted audit record must
    /// surface, not silently degrade to an empty detail object.
    ///
    /// Shared by every audit_events read path in this adapter (AuditRepository
    /// query/since_timestamp and the JudgmentLedgerRepository source-2 query)
    /// so compressed rows decompress transparently everywhere.
    pub(crate) fn into_event(self) -> Result<AuditEvent> {
        let detail = match self.detail_zstd {
            Some(z) if !z.is_empty() => {
                let raw =
                    zstd::decode_all(z.as_slice()).context("decompress audit detail (zstd)")?;
                serde_json::from_slice(&raw).context("parse compressed audit detail")?
            }
            _ => serde_json::from_str(&self.detail)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        };
        Ok(AuditEvent {
            id: Id::new(self.id),
            event_type: AuditEventType::from_str(&self.event_type),
            agent_id: self.agent_id.map(Id::new),
            user_id: self.user_id.map(Id::new),
            session_id: self.session_id,
            workspace_id: self.workspace_id.map(Id::new),
            tenant_id: self.tenant_id,
            repo_id: self.repo_id.map(Id::new),
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            outcome: AuditOutcome::from_str(&self.outcome).unwrap_or(AuditOutcome::Success),
            detail,
            source_ip: self.source_ip,
            user_agent: self.user_agent,
            timestamp: self.timestamp as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = audit_events)]
struct AuditEventRecord<'a> {
    id: &'a str,
    event_type: String,
    agent_id: Option<&'a str>,
    user_id: Option<&'a str>,
    session_id: Option<&'a str>,
    workspace_id: Option<&'a str>,
    tenant_id: Option<&'a str>,
    repo_id: Option<&'a str>,
    resource_type: &'a str,
    resource_id: Option<&'a str>,
    outcome: &'a str,
    detail: String,
    detail_zstd: Option<Vec<u8>>,
    source_ip: Option<&'a str>,
    user_agent: Option<&'a str>,
    timestamp: i64,
}

#[derive(QueryableByName)]
struct EventTypeStat {
    #[diesel(sql_type = Text)]
    event_type: String,
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

fn to_record(e: &AuditEvent) -> AuditEventRecord<'_> {
    let event_type_str = e.event_type.as_str();
    AuditEventRecord {
        id: e.id.as_str(),
        event_type: event_type_str,
        agent_id: e.agent_id.as_ref().map(|id| id.as_str()),
        user_id: e.user_id.as_ref().map(|id| id.as_str()),
        session_id: e.session_id.as_deref(),
        workspace_id: e.workspace_id.as_ref().map(|id| id.as_str()),
        tenant_id: e.tenant_id.as_deref(),
        repo_id: e.repo_id.as_ref().map(|id| id.as_str()),
        resource_type: &e.resource_type,
        resource_id: e.resource_id.as_deref(),
        outcome: e.outcome.as_str(),
        detail: serde_json::to_string(&e.detail).unwrap_or_else(|_| "{}".to_string()),
        // New rows are never pre-compressed; the log-compression job
        // picks them up once they age past 7 days.
        detail_zstd: None,
        source_ip: e.source_ip.as_deref(),
        user_agent: e.user_agent.as_deref(),
        timestamp: e.timestamp as i64,
    }
}

#[async_trait]
impl AuditRepository for PgStorage {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mut e = event.clone();
        // Tenant scoping (admin-panel.md §Audit Log Viewer): derive the
        // scope server-side when the emitter did not — the workspace's
        // owning tenant, or the acting user's tenant binding. A record
        // that cannot be scoped keeps NULL (never attributed to a tenant
        // it did not come from).
        if e.tenant_id.is_none() {
            if let Some(ws_id) = e.workspace_id.as_ref() {
                e.tenant_id = self.tenant_of_workspace(&ws_id.to_string()).await?;
            }
            if e.tenant_id.is_none() {
                if let Some(user_id) = e.user_id.as_ref() {
                    e.tenant_id = self.tenant_of_user(&user_id.to_string()).await?;
                }
            }
        }
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = to_record(&e);
            diesel::insert_into(audit_events::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert audit_event")?;
            Ok(())
        })
        .await?
    }

    async fn query(&self, filter: &AuditQueryFilter) -> Result<Vec<AuditEvent>> {
        let pool = Arc::clone(&self.pool);
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            query_rows(&mut conn, &filter)
        })
        .await?
    }

    async fn count(&self) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = audit_events::table
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count audit_events")?;
            Ok(n as u64)
        })
        .await?
    }

    async fn count_filtered(&self, filter: &AuditQueryFilter) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            count_rows(&mut conn, &filter)
        })
        .await?
    }

    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = diesel::sql_query(
                "SELECT event_type, COUNT(*) as cnt \
                 FROM audit_events GROUP BY event_type ORDER BY cnt DESC",
            )
            .load::<EventTypeStat>(&mut *conn)
            .context("stats_by_type")?;
            Ok(rows
                .into_iter()
                .map(|r| (r.event_type, r.cnt as u64))
                .collect())
        })
        .await?
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = audit_events::table
                .filter(audit_events::timestamp.gt(since as i64))
                .order(audit_events::timestamp.asc())
                .limit(limit as i64)
                .load::<AuditEventRow>(&mut *conn)
                .context("since_timestamp audit_events")?;
            rows.into_iter()
                .map(AuditEventRow::into_event)
                .collect::<Result<Vec<_>>>()
        })
        .await?
    }

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = diesel::delete(audit_events::table)
                .filter(audit_events::timestamp.lt(cutoff_secs as i64))
                .execute(&mut *conn)
                .context("delete old audit_events")?;
            Ok(n as u64)
        })
        .await?
    }

    async fn compress_details_before(
        &self,
        cutoff_secs: u64,
        limit: usize,
    ) -> Result<CompressionStats> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<CompressionStats> {
            let mut conn = pool.get().context("get db connection")?;
            // Candidates: not yet compressed, older than the cutoff.
            let rows: Vec<(String, String)> = audit_events::table
                .filter(audit_events::detail_zstd.is_null())
                .filter(audit_events::timestamp.lt(cutoff_secs as i64))
                .order(audit_events::timestamp.asc())
                .limit(limit as i64)
                .select((audit_events::id, audit_events::detail))
                .load(&mut *conn)
                .context("select audit_events for compression")?;

            let mut stats = CompressionStats::default();
            for (id, detail) in rows {
                let compressed =
                    zstd::encode_all(detail.as_bytes(), 3).context("zstd compress audit detail")?;
                // The NULL guard makes the update idempotent under
                // concurrency: only the first writer flips the row.
                let n = diesel::update(audit_events::table)
                    .filter(audit_events::id.eq(&id))
                    .filter(audit_events::detail_zstd.is_null())
                    .set((
                        audit_events::detail.eq(""),
                        audit_events::detail_zstd.eq(Some(compressed.clone())),
                    ))
                    .execute(&mut *conn)
                    .context("compress audit detail")?;
                if n == 1 {
                    stats.compressed += 1;
                    stats.bytes_before += detail.len() as u64;
                    stats.bytes_after += compressed.len() as u64;
                }
            }
            Ok(stats)
        })
        .await?
    }
}

type BoxedAuditQuery =
    diesel::dsl::IntoBoxed<'static, crate::schema::audit_events::table, diesel::pg::Pg>;

// Portable SQL LOWER() for the case-insensitive free-text search (both
// SQLite and PostgreSQL implement LOWER over text).
diesel::define_sql_function! {
    fn lower(x: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

/// Mirror of the sqlite adapter's filter builder — the same port contract
/// (every `AuditQueryFilter` dimension, including `repo_id`) compiled to
/// SQL. Shared by `query` and `count_filtered` so the two can never
/// disagree on what matches.
impl PgStorage {
    /// Resolve a workspace's owning tenant (audit tenant scoping,
    /// admin-panel.md §Audit Log Viewer). `Ok(None)` when the workspace row
    /// does not exist — the scope cannot be determined, so it is not
    /// attributed (skip, never fabricate).
    async fn tenant_of_workspace(&self, workspace_id: &str) -> Result<Option<String>> {
        use crate::schema::workspaces;
        let pool = Arc::clone(&self.pool);
        let ws = workspace_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            Ok(workspaces::table
                .filter(workspaces::id.eq(ws))
                .select(workspaces::tenant_id)
                .first::<String>(&mut conn)
                .optional()?)
        })
        .await?
    }

    /// Resolve a user's tenant binding (same contract as
    /// [`Self::tenant_of_workspace`]).
    async fn tenant_of_user(&self, user_id: &str) -> Result<Option<String>> {
        use crate::schema::users;
        let pool = Arc::clone(&self.pool);
        let uid = user_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            Ok(users::table
                .filter(users::id.eq(uid))
                .select(users::tenant_id)
                .first::<Option<String>>(&mut conn)
                .optional()?
                .flatten())
        })
        .await?
    }
}

/// Escape LIKE wildcards in a free-text search term so `q` matches
/// literally (`%`/`_`/`\\` in the term cannot broaden or break the match).
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether a row's detail payload matches the free-text term
/// (case-insensitive substring over the serialized detail JSON).
/// Compressed rows are decompressed before matching — the same JSON the
/// uncompressed `detail` column would hold — so search works uniformly
/// across both storage states (observability.md §Log Retention &
/// Compression).
fn detail_matches(row: &AuditEventRow, term: &str) -> bool {
    let needle = term.to_ascii_lowercase();
    let haystack = match &row.detail_zstd {
        Some(z) if !z.is_empty() => match zstd::decode_all(z.as_slice()) {
            Ok(raw) => String::from_utf8_lossy(&raw).to_string(),
            Err(_) => return false,
        },
        _ => row.detail.clone(),
    };
    haystack.to_ascii_lowercase().contains(&needle)
}

/// Fetch rows matching `filter`, honoring the free-text `q` term over both
/// the raw `detail` column (SQL LIKE — enforced in SQL, the fast path) and
/// compressed rows (decompressed + matched in the adapter, since the
/// compressed payload is not LIKE-able). Paging keeps the result correct
/// without an unbounded scan.
fn query_rows(
    conn: &mut diesel::PgConnection,
    filter: &AuditQueryFilter,
) -> Result<Vec<AuditEvent>> {
    // Tenant scoping (admin-panel.md §Audit Log Viewer) is applied here,
    // lexically at the read entry point, so a tenant-scoped caller's query
    // visibly filters by tenant_id (hierarchy-enforcement.md §3); the
    // remaining structured filters ride in apply_filters. Unattributed
    // legacy rows (NULL tenant, created before tenant attribution
    // existed) are visible to unscoped internal reads only — never to a
    // tenant-scoped admin query, which would attribute them to a tenant
    // they did not come from.
    let tenant_scoped = |mut query: BoxedAuditQuery| -> BoxedAuditQuery {
        if let Some(t) = &filter.tenant_id {
            query = query.filter(audit_events::tenant_id.eq(t.clone()));
        }
        query
    };
    let needs_q = filter
        .q
        .as_deref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    if !needs_q {
        let rows = apply_filters(tenant_scoped(audit_events::table.into_boxed()), filter)
            .order(audit_events::timestamp.desc())
            .limit(filter.limit as i64)
            .load::<AuditEventRow>(conn)
            .context("query audit_events")?;
        return rows
            .into_iter()
            .map(AuditEventRow::into_event)
            .collect::<Result<Vec<_>>>();
    }

    let term = filter.q.as_deref().unwrap_or_default().trim().to_string();
    let mut matched: Vec<AuditEventRow> = Vec::new();
    let page = (filter.limit.max(100) as i64).min(1_000);
    let mut offset: i64 = 0;
    loop {
        let rows = apply_filters(tenant_scoped(audit_events::table.into_boxed()), filter)
            .order(audit_events::timestamp.desc())
            .limit(page)
            .offset(offset)
            .load::<AuditEventRow>(conn)
            .context("query audit_events (free-text page)")?;
        let fetched = rows.len() as i64;
        for row in rows {
            if detail_matches(&row, &term) {
                matched.push(row);
            }
        }
        offset += fetched;
        if matched.len() >= filter.limit || fetched < page {
            break;
        }
    }
    matched.truncate(filter.limit);
    matched
        .into_iter()
        .map(AuditEventRow::into_event)
        .collect::<Result<Vec<_>>>()
}

/// Count rows matching `filter` including the free-text `q` term (same
/// matching semantics as [`query_rows`]; `limit` deliberately not applied —
/// the count covers the full filtered set, per the port contract).
fn count_rows(conn: &mut diesel::PgConnection, filter: &AuditQueryFilter) -> Result<u64> {
    // Tenant scoping lives here, lexically at the read entry point (see
    // query_rows); apply_filters handles the remaining structured filters.
    let tenant_scoped = |mut query: BoxedAuditQuery| -> BoxedAuditQuery {
        if let Some(t) = &filter.tenant_id {
            query = query.filter(audit_events::tenant_id.eq(t.clone()));
        }
        query
    };
    let needs_q = filter
        .q
        .as_deref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    if !needs_q {
        let n = apply_filters(tenant_scoped(audit_events::table.into_boxed()), filter)
            .count()
            .get_result::<i64>(conn)
            .context("count audit_events (filtered)")?;
        return Ok(n as u64);
    }
    let term = filter.q.as_deref().unwrap_or_default().trim().to_string();
    let mut total: u64 = 0;
    let page: i64 = 500;
    let mut offset: i64 = 0;
    loop {
        let rows = apply_filters(tenant_scoped(audit_events::table.into_boxed()), filter)
            .order(audit_events::timestamp.desc())
            .limit(page)
            .offset(offset)
            .load::<AuditEventRow>(conn)
            .context("count audit_events (free-text page)")?;
        let fetched = rows.len() as i64;
        total += rows.iter().filter(|r| detail_matches(r, &term)).count() as u64;
        offset += fetched;
        if fetched < page {
            break;
        }
    }
    Ok(total)
}

fn apply_filters(mut query: BoxedAuditQuery, filter: &AuditQueryFilter) -> BoxedAuditQuery {
    if let Some(s) = filter.since {
        query = query.filter(audit_events::timestamp.ge(s as i64));
    }
    if let Some(u) = filter.until {
        query = query.filter(audit_events::timestamp.le(u as i64));
    }
    if let Some(a) = &filter.agent_id {
        query = query.filter(audit_events::agent_id.eq(a.clone()));
    }
    if let Some(et) = &filter.event_type {
        query = query.filter(audit_events::event_type.eq(et.clone()));
    }
    if let Some(w) = &filter.workspace_id {
        query = query.filter(audit_events::workspace_id.eq(w.clone()));
    }
    // NOTE: tenant scoping is deliberately NOT applied here — it lives
    // lexically in the read entry points (query_rows / count_rows), which
    // call this helper with an already tenant-scoped base query.
    if let Some(u) = &filter.user_id {
        query = query.filter(audit_events::user_id.eq(u.clone()));
    }
    if let Some(r) = &filter.repo_id {
        query = query.filter(audit_events::repo_id.eq(r.clone()));
    }
    if let Some(rt) = &filter.resource_type {
        query = query.filter(audit_events::resource_type.eq(rt.clone()));
    }
    if let Some(o) = filter.outcome {
        query = query.filter(audit_events::outcome.eq(o.as_str()));
    }
    if let Some(q) = filter.q.as_deref() {
        let term = q.trim();
        if !term.is_empty() {
            use diesel::expression_methods::EscapeExpressionMethods;
            // Case-insensitive on both backends: LOWER(detail) LIKE the
            // lowercased pattern (SQLite's LIKE is case-insensitive by
            // default, PostgreSQL's is not — lowering both sides makes the
            // behavior identical). Wildcards in the term are escaped so it
            // matches literally. Compressed rows (detail blanked by the
            // log-compression job) are decompressed and matched in the
            // adapter — see `query_rows`.
            let pattern = format!("%{}%", escape_like(&term.to_ascii_lowercase()));
            query = query.filter(
                lower(audit_events::detail)
                    .like(pattern)
                    .escape('\\')
                    .or(audit_events::detail_zstd.is_not_null()),
            );
        }
    }
    query
}

#[cfg(test)]
mod tests {
    //! SQL-shape regression tests for the TASK-273 audit filter dimensions
    //! (tenant scoping + free-text q). The repo has no PG server harness
    //! (postgres/search.rs + user_profile.rs precedent), so these pin the
    //! generated statement shape — the surface where a PG-side drift from
    //! the sqlite sibling (which carries the behavioral tests against a
    //! real SQLite file) would otherwise surface only in production. The
    //! behavioral semantics (tenant union, LIKE escaping, zstd
    //! decompression, paging) are exercised by the sqlite module's suite.

    use super::*;

    /// Mirrors the query_rows / count_rows entry-point scoping (tenant
    /// filter first, then apply_filters) so the pinned SQL shape is exactly
    /// what the served queries generate.
    fn sql(filter: &AuditQueryFilter) -> String {
        let mut base = audit_events::table.into_boxed();
        if let Some(t) = &filter.tenant_id {
            base = base.filter(audit_events::tenant_id.eq(t.clone()));
        }
        diesel::debug_query::<diesel::pg::Pg, _>(&apply_filters(base, filter)).to_string()
    }

    fn all() -> AuditQueryFilter {
        AuditQueryFilter {
            limit: 100,
            ..Default::default()
        }
    }

    #[test]
    fn tenant_filter_is_bare_equality_in_sql() {
        // The caller's tenant ONLY: the predicate must be a bare
        // "tenant_id = $n" equality. A (tenant_id = $n OR tenant_id IS
        // NULL) union would leak unattributed legacy rows to every
        // tenant-scoped admin query — the exact contradiction of the
        // AuditEvent::tenant_id contract the task-273 review round fixed.
        let filtered = sql(&AuditQueryFilter {
            tenant_id: Some("tenant-1".to_string()),
            ..all()
        });
        assert!(
            filtered.contains("\"tenant_id\" = $"),
            "tenant scoping predicate missing: {filtered}"
        );
        assert!(
            !filtered.contains("\"tenant_id\" IS NULL"),
            "tenant scoping must not union NULL-tenant rows in: {filtered}"
        );

        let unfiltered = sql(&all());
        assert!(
            !unfiltered.contains("tenant_id = $"),
            "no tenant filter requested but WHERE carries one: {unfiltered}"
        );
    }

    #[test]
    fn free_text_filter_compiles_to_lower_like_with_escape() {
        // The q fast path must be enforced IN SQL: LOWER(detail) LIKE the
        // lowercased pattern with '\\' escaping, OR-unioned with
        // detail_zstd IS NOT NULL (compressed rows are matched in the
        // adapter after decompression).
        let filtered = sql(&AuditQueryFilter {
            q: Some("needle".to_string()),
            ..all()
        });
        assert!(
            filtered.contains("LIKE"),
            "free-text q must be a SQL LIKE predicate: {filtered}"
        );
        assert!(
            filtered.to_ascii_lowercase().contains("lower("),
            "LIKE must compare LOWER(detail) for case-insensitivity: {filtered}"
        );
        assert!(
            filtered.contains("\"detail_zstd\" IS NOT NULL"),
            "compressed rows must pass the SQL filter for adapter-side matching: {filtered}"
        );

        let unfiltered = sql(&all());
        assert!(
            !unfiltered.contains("LIKE"),
            "no q requested but WHERE carries a LIKE: {unfiltered}"
        );
    }

    #[test]
    fn free_text_filter_combines_with_tenant_scoping() {
        // q + tenant_id compose: both predicates present in one statement.
        let both = sql(&AuditQueryFilter {
            q: Some("needle".to_string()),
            tenant_id: Some("tenant-1".to_string()),
            ..all()
        });
        assert!(both.contains("LIKE"));
        assert!(both.contains("\"tenant_id\" = $"));
        assert!(
            !both.contains("\"tenant_id\" IS NULL"),
            "tenant scoping must stay a bare equality when combined with q: {both}"
        );
    }

    #[test]
    fn escape_like_escapes_all_wildcards() {
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("back\\slash"), "back\\\\slash");
        // The escaped term still contains the original characters — the
        // escape only inserts the backslash guard before each wildcard.
        assert_eq!(escape_like("50%").replace("\\", ""), "50%");
    }

    #[test]
    fn detail_matches_over_compressed_and_plain_rows() {
        fn row_with(detail: String, zstd_bytes: Option<Vec<u8>>) -> AuditEventRow {
            AuditEventRow {
                id: "e".to_string(),
                event_type: "file_access".to_string(),
                agent_id: None,
                user_id: None,
                session_id: None,
                workspace_id: None,
                tenant_id: None,
                repo_id: None,
                resource_type: "agent".to_string(),
                resource_id: None,
                outcome: "success".to_string(),
                detail,
                detail_zstd: zstd_bytes,
                source_ip: None,
                user_agent: None,
                timestamp: 0,
            }
        }
        // Plain row: case-insensitive substring over the raw detail.
        let plain = row_with(r#"{"needle":"FindMe"}"#.to_string(), None);
        assert!(detail_matches(&plain, "findme"));
        assert!(!detail_matches(&plain, "absent"));

        // Compressed row: matched over the DECOMPRESSED payload.
        let payload = r#"{"needle":"compressed-findme"}"#;
        let compressed = zstd::encode_all(payload.as_bytes(), 3).unwrap();
        let zrow = row_with(String::new(), Some(compressed));
        assert!(detail_matches(&zrow, "compressed-findme"));
        assert!(!detail_matches(&zrow, "plain-only"));

        // A row that fails to decompress never matches (fail closed).
        let corrupt = row_with(String::new(), Some(vec![0xff, 0xfe, 0xfd]));
        assert!(!detail_matches(&corrupt, "anything"));
    }
}
