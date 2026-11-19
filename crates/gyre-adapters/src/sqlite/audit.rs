use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_ports::{AuditQueryFilter, AuditRepository, CompressionStats};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::audit_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
    AuditEventRecord {
        id: e.id.as_str(),
        event_type: e.event_type.as_str(),
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

impl SqliteStorage {
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

#[async_trait]
impl AuditRepository for SqliteStorage {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mut e = event.clone();
        // Tenant scoping (admin-panel.md §Audit Log Viewer): derive the
        // scope server-side when the emitter did not — the workspace's
        // owning tenant, or the acting user's tenant binding. A record
        // that cannot be scoped keeps NULL (visible to unscoped internal
        // reads only, never attributed to a tenant it did not come from).
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
            diesel::insert_into(audit_events::table)
                .values(&to_record(&e))
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
    diesel::dsl::IntoBoxed<'static, crate::schema::audit_events::table, diesel::sqlite::Sqlite>;

// Portable SQL LOWER() for the case-insensitive free-text search (both
// SQLite and PostgreSQL implement LOWER over text).
diesel::define_sql_function! {
    fn lower(x: diesel::sql_types::Text) -> diesel::sql_types::Text;
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

/// Whether a materialized event's detail payload matches the free-text
/// term (case-insensitive substring over the serialized detail JSON).
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
/// without an unbounded scan: pages are fetched newest-first until
/// `filter.limit` matches are collected or the candidate set is exhausted.
fn query_rows(conn: &mut SqliteConnection, filter: &AuditQueryFilter) -> Result<Vec<AuditEvent>> {
    if filter.q.is_none() {
        let rows = apply_filters(audit_events::table.into_boxed(), filter)
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
    if term.is_empty() {
        let rows = apply_filters(audit_events::table.into_boxed(), filter)
            .order(audit_events::timestamp.desc())
            .limit(filter.limit as i64)
            .load::<AuditEventRow>(conn)
            .context("query audit_events")?;
        return rows
            .into_iter()
            .map(AuditEventRow::into_event)
            .collect::<Result<Vec<_>>>();
    }

    let mut matched: Vec<AuditEventRow> = Vec::new();
    let page = (filter.limit.max(100) as i64).min(1_000);
    let mut offset: i64 = 0;
    loop {
        let rows = apply_filters(audit_events::table.into_boxed(), filter)
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
fn count_rows(conn: &mut SqliteConnection, filter: &AuditQueryFilter) -> Result<u64> {
    if filter.q.is_none() {
        let n = apply_filters(audit_events::table.into_boxed(), filter)
            .count()
            .get_result::<i64>(conn)
            .context("count audit_events (filtered)")?;
        return Ok(n as u64);
    }
    let term = filter.q.as_deref().unwrap_or_default().trim().to_string();
    if term.is_empty() {
        let n = apply_filters(audit_events::table.into_boxed(), filter)
            .count()
            .get_result::<i64>(conn)
            .context("count audit_events (filtered)")?;
        return Ok(n as u64);
    }
    let mut total: u64 = 0;
    let page: i64 = 500;
    let mut offset: i64 = 0;
    loop {
        let rows = apply_filters(audit_events::table.into_boxed(), filter)
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
    // Tenant scoping (admin-panel.md §Audit Log Viewer): rows scoped to
    // the caller's tenant, plus unscoped legacy rows (NULL tenant — created
    // before tenant attribution existed); another tenant's events are
    // never returned.
    if let Some(t) = &filter.tenant_id {
        query = query.filter(
            audit_events::tenant_id
                .eq(t.clone())
                .or(audit_events::tenant_id.is_null()),
        );
    }
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
    use super::*;
    use gyre_domain::AuditEventType;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_event(id: &str, agent: &str, et: AuditEventType, ts: u64) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            et,
            Some(Id::new(agent)),
            None,
            None,
            None,
            None,
            "agent".to_string(),
            Some(agent.to_string()),
            AuditOutcome::Success,
            serde_json::json!({ "path": "/tmp/test", "action": "read", "pid": 1000 }),
            None,
            None,
            ts,
        )
    }

    async fn record(s: &SqliteStorage, e: &AuditEvent) {
        AuditRepository::record(s, e).await.unwrap();
    }

    fn all() -> AuditQueryFilter {
        AuditQueryFilter {
            limit: 100,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn audit_record_and_query_all() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await;
        record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200),
        )
        .await;
        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn audit_query_by_agent() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await;
        record(
            &s,
            &make_event("e2", "agent-2", AuditEventType::ProcessExec, 200),
        )
        .await;
        let results = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                agent_id: Some("agent-1".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, Some(Id::new("agent-1")));
    }

    #[tokio::test]
    async fn audit_query_by_event_type() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await;
        record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200),
        )
        .await;
        let results = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                event_type: Some("file_access".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, AuditEventType::FileAccess);
    }

    #[tokio::test]
    async fn audit_query_by_workspace_user_resource_outcome() {
        // Each new filter dimension independently narrows the result set.
        let (_tmp, s) = setup();
        let mut e1 = make_event("e1", "agent-1", AuditEventType::FileAccess, 100);
        e1.workspace_id = Some(Id::new("ws-1"));
        e1.user_id = Some(Id::new("user-1"));
        e1.outcome = AuditOutcome::Failure;
        record(&s, &e1).await;
        let mut e2 = make_event("e2", "agent-2", AuditEventType::FileAccess, 200);
        e2.workspace_id = Some(Id::new("ws-2"));
        e2.user_id = Some(Id::new("user-2"));
        e2.outcome = AuditOutcome::Success;
        e2.resource_type = "container".to_string();
        record(&s, &e2).await;

        let by_ws = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                workspace_id: Some("ws-1".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_ws.len(), 1);
        assert_eq!(by_ws[0].id, Id::new("e1"));

        let by_user = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                user_id: Some("user-2".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_user.len(), 1);
        assert_eq!(by_user[0].id, Id::new("e2"));

        let by_rt = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                resource_type: Some("container".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_rt.len(), 1);
        assert_eq!(by_rt[0].id, Id::new("e2"));

        let by_outcome = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                outcome: Some(AuditOutcome::Failure),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_outcome.len(), 1);
        assert_eq!(by_outcome[0].id, Id::new("e1"));
    }

    #[tokio::test]
    async fn audit_server_initiated_event_has_null_agent() {
        // agent_id NULL must persist and round-trip (spec: server-initiated).
        let (_tmp, s) = setup();
        let e = AuditEvent::new(
            Id::new("e1"),
            AuditEventType::Custom("auth_failure".to_string()),
            None,
            Some(Id::new("user-1")),
            None,
            None,
            None,
            "user".to_string(),
            Some("user-1".to_string()),
            AuditOutcome::Failure,
            serde_json::json!({ "rejection_reason": "revoked" }),
            Some("10.0.0.1".to_string()),
            Some("curl/8.0".to_string()),
            100,
        );
        record(&s, &e).await;
        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, None);
        assert_eq!(results[0].user_id, Some(Id::new("user-1")));
        assert_eq!(results[0].source_ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(results[0].user_agent.as_deref(), Some("curl/8.0"));
        assert_eq!(results[0].outcome, AuditOutcome::Failure);
    }

    #[tokio::test]
    async fn audit_query_since_until() {
        let (_tmp, s) = setup();
        for i in 1u64..=5 {
            record(
                &s,
                &make_event(
                    &format!("e{}", i),
                    "agent-1",
                    AuditEventType::Syscall,
                    i * 100,
                ),
            )
            .await;
        }
        let results = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                since: Some(200),
                until: Some(400),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results
            .iter()
            .all(|e| e.timestamp >= 200 && e.timestamp <= 400));
    }

    #[tokio::test]
    async fn audit_query_by_repo_and_count_filtered() {
        // `repo_id` narrows both `query` and `count_filtered`; the count
        // ignores `limit` (port contract) so a total is never bounded by
        // the page size used to fetch the list.
        let (_tmp, s) = setup();
        let mut e1 = make_event("e1", "agent-1", AuditEventType::FileAccess, 100);
        e1.repo_id = Some(Id::new("repo-1"));
        let mut e2 = make_event("e2", "agent-1", AuditEventType::Syscall, 200);
        e2.repo_id = Some(Id::new("repo-1"));
        let mut e3 = make_event("e3", "agent-2", AuditEventType::Syscall, 300);
        e3.repo_id = Some(Id::new("repo-2"));
        record(&s, &e1).await;
        record(&s, &e2).await;
        record(&s, &e3).await;

        let filter = AuditQueryFilter {
            repo_id: Some("repo-1".to_string()),
            // Deliberately tiny: count_filtered must ignore it.
            limit: 1,
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(results.len(), 1, "list honors limit");
        assert!(results.iter().all(|e| e.repo_id == Some(Id::new("repo-1"))));
        let count = AuditRepository::count_filtered(&s, &filter).await.unwrap();
        assert_eq!(
            count, 2,
            "count covers the full filtered set, ignoring limit"
        );

        // No repo filter → counts everything.
        let count = AuditRepository::count_filtered(&s, &all()).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn audit_count() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await;
        record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::Syscall, 200),
        )
        .await;
        let count = AuditRepository::count(&s).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn audit_stats_by_type() {
        let (_tmp, s) = setup();
        record(&s, &make_event("e1", "a1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "a1", AuditEventType::FileAccess, 200)).await;
        record(
            &s,
            &make_event("e3", "a1", AuditEventType::NetworkConnect, 300),
        )
        .await;
        let stats = AuditRepository::stats_by_type(&s).await.unwrap();
        let fa = stats.iter().find(|(t, _)| t == "file_access").unwrap();
        assert_eq!(fa.1, 2);
        let nc = stats.iter().find(|(t, _)| t == "network_connect").unwrap();
        assert_eq!(nc.1, 1);
    }

    #[tokio::test]
    async fn audit_since_timestamp() {
        let (_tmp, s) = setup();
        for i in 1u64..=5 {
            record(
                &s,
                &make_event(&format!("e{}", i), "a1", AuditEventType::Syscall, i * 100),
            )
            .await;
        }
        let results = AuditRepository::since_timestamp(&s, 300, 10).await.unwrap();
        assert_eq!(results.len(), 2); // timestamps 400, 500
        assert!(results.iter().all(|e| e.timestamp > 300));
    }

    #[tokio::test]
    async fn audit_custom_event_type() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event(
                "e1",
                "a1",
                AuditEventType::Custom("container_escape".to_string()),
                100,
            ),
        )
        .await;
        let results = AuditRepository::query(
            &s,
            &AuditQueryFilter {
                event_type: Some("container_escape".to_string()),
                ..all()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].event_type,
            AuditEventType::Custom("container_escape".to_string())
        );
    }

    #[tokio::test]
    async fn audit_delete_older_than_purges_old_keeps_new() {
        let (_tmp, s) = setup();
        record(
            &s,
            &make_event("old", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await;
        record(
            &s,
            &make_event("new", "agent-1", AuditEventType::FileAccess, 200),
        )
        .await;

        let deleted = AuditRepository::delete_older_than(&s, 150).await.unwrap();
        assert_eq!(deleted, 1);

        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("new"));

        // Second run is idempotent.
        assert_eq!(
            AuditRepository::delete_older_than(&s, 150).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn audit_compress_details_before_compresses_only_old_uncompressed() {
        let (_tmp, s) = setup();
        // Repetitive JSON compresses well — the byte accounting below
        // relies on a real ratio.
        let detail = serde_json::json!({
            "path": "/tmp/some/long/path/repeated/in/every/event",
            "action": "read",
            "pid": 1000,
            "padding": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        let mut old = make_event("old", "agent-1", AuditEventType::FileAccess, 100);
        old.detail = detail.clone();
        let mut new = make_event("new", "agent-1", AuditEventType::FileAccess, 200);
        new.detail = detail.clone();
        record(&s, &old).await;
        record(&s, &new).await;

        let stats = AuditRepository::compress_details_before(&s, 150, 100)
            .await
            .unwrap();
        assert_eq!(stats.compressed, 1, "only the old event is compressed");
        assert!(
            stats.bytes_before > stats.bytes_after,
            "zstd must shrink the detail JSON"
        );

        // Transparent decompression on query: the old event keeps its
        // original detail payload, event fields intact.
        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 2);
        for e in &results {
            assert_eq!(
                e.detail, detail,
                "detail must round-trip regardless of compression"
            );
        }
        let old = results.iter().find(|e| e.id.as_str() == "old").unwrap();
        assert_eq!(old.event_type, AuditEventType::FileAccess);
        assert_eq!(old.agent_id, Some(Id::new("agent-1")));
    }

    #[tokio::test]
    async fn audit_compress_details_is_idempotent_and_limit_bounded() {
        let (_tmp, s) = setup();
        for i in 0..5 {
            record(
                &s,
                &make_event(
                    &format!("e{}", i),
                    "agent-1",
                    AuditEventType::FileAccess,
                    100 + i,
                ),
            )
            .await;
        }

        // limit bounds one pass to 2 rows.
        let first = AuditRepository::compress_details_before(&s, 1000, 2)
            .await
            .unwrap();
        assert_eq!(first.compressed, 2);
        // Second pass with the same limit picks up two more rows...
        let second = AuditRepository::compress_details_before(&s, 1000, 2)
            .await
            .unwrap();
        assert_eq!(second.compressed, 2);
        // ...the third pass gets the last remaining row...
        let third = AuditRepository::compress_details_before(&s, 1000, 2)
            .await
            .unwrap();
        assert_eq!(third.compressed, 1);
        // ...and a fourth pass finds nothing new (idempotent).
        let fourth = AuditRepository::compress_details_before(&s, 1000, 2)
            .await
            .unwrap();
        assert_eq!(fourth.compressed, 0);
        assert_eq!(fourth.bytes_before, 0);
        assert_eq!(fourth.bytes_after, 0);

        // All rows still readable with their original detail.
        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 5);
        for e in &results {
            assert_eq!(
                e.detail,
                serde_json::json!({ "path": "/tmp/test", "action": "read", "pid": 1000 })
            );
        }
    }

    #[tokio::test]
    async fn audit_since_timestamp_decompresses_compressed_events() {
        // SIEM forwarding reads exclusively through since_timestamp; a
        // compressed event must arrive with its original detail.
        let (_tmp, s) = setup();
        let mut e = make_event("e1", "agent-1", AuditEventType::FileAccess, 100);
        e.detail = serde_json::json!({ "path": "/etc/shadow", "mode": "read" });
        record(&s, &e).await;

        let stats = AuditRepository::compress_details_before(&s, 200, 10)
            .await
            .unwrap();
        assert_eq!(stats.compressed, 1);

        let events = AuditRepository::since_timestamp(&s, 50, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail, e.detail);
    }

    // ── TASK-273: tenant scoping + free-text q search (SQL path) ─────────────

    #[tokio::test]
    async fn audit_query_tenant_scoped_with_null_legacy_union() {
        // Tenant scoping: rows of the caller's tenant plus unscoped legacy
        // rows (NULL tenant) are returned; ANOTHER tenant's rows never are.
        let (_tmp, s) = setup();

        let mut t1 = make_event("e-t1", "a1", AuditEventType::FileAccess, 100);
        t1.tenant_id = Some("tenant-1".to_string());
        record(&s, &t1).await;

        let mut t2 = make_event("e-t2", "a2", AuditEventType::FileAccess, 200);
        t2.tenant_id = Some("tenant-2".to_string());
        record(&s, &t2).await;

        // Legacy row: created before tenant attribution existed — no
        // workspace/user the adapter could derive a tenant from, so it is
        // stored with NULL tenant_id.
        let legacy = make_event("e-legacy", "a3", AuditEventType::FileAccess, 300);
        record(&s, &legacy).await;

        let filter = AuditQueryFilter {
            tenant_id: Some("tenant-1".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        let ids: Vec<&str> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["e-legacy", "e-t1"],
            "own tenant + NULL-legacy union, newest first"
        );

        // count_filtered agrees with the query.
        assert_eq!(
            AuditRepository::count_filtered(&s, &filter).await.unwrap(),
            2
        );

        // A tenant-2 caller sees only its own row plus the legacy row.
        let filter = AuditQueryFilter {
            tenant_id: Some("tenant-2".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        let ids: Vec<&str> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["e-legacy", "e-t2"]);
    }

    #[tokio::test]
    async fn audit_free_text_search_matches_detail_only() {
        let (_tmp, s) = setup();
        let mut hit = make_event("e-hit", "a1", AuditEventType::FileAccess, 100);
        hit.detail =
            serde_json::json!({ "secret_name": "claude_max_refresh_token", "scope": "workspace" });
        let mut other = make_event("e-other", "a2", AuditEventType::FileAccess, 200);
        other.detail = serde_json::json!({ "secret_name": "gcp_sa_key", "scope": "repo" });
        record(&s, &hit).await;
        record(&s, &other).await;

        let filter = AuditQueryFilter {
            q: Some("claude_max".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(results.len(), 1, "only the matching event is returned");
        assert_eq!(results[0].id, Id::new("e-hit"));
        assert_eq!(
            AuditRepository::count_filtered(&s, &filter).await.unwrap(),
            1
        );

        // Case-insensitive on the serialized detail payload.
        let filter = AuditQueryFilter {
            q: Some("CLAUDE_MAX".to_string()),
            ..all()
        };
        assert_eq!(AuditRepository::query(&s, &filter).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn audit_free_text_search_escapes_like_wildcards() {
        // `%`/`_` in the term must match LITERALLY — they cannot broaden the
        // match to every row.
        let (_tmp, s) = setup();
        let mut pct = make_event("e-pct", "a1", AuditEventType::FileAccess, 100);
        pct.detail = serde_json::json!({ "quota": "100%_of_budget" });
        let mut plain = make_event("e-plain", "a2", AuditEventType::FileAccess, 200);
        plain.detail = serde_json::json!({ "quota": "half of budget" });
        record(&s, &pct).await;
        record(&s, &plain).await;

        // A bare `%` matches only the row whose detail literally contains
        // a percent sign — not everything.
        let filter = AuditQueryFilter {
            q: Some("%".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("e-pct"));

        // `_` likewise matches literally, not "any character".
        let filter = AuditQueryFilter {
            q: Some("100%_of".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("e-pct"));

        // A term matching nothing returns nothing.
        let filter = AuditQueryFilter {
            q: Some("does-not-appear-anywhere".to_string()),
            ..all()
        };
        assert_eq!(AuditRepository::query(&s, &filter).await.unwrap().len(), 0);
        assert_eq!(
            AuditRepository::count_filtered(&s, &filter).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn audit_free_text_search_matches_zstd_compressed_detail() {
        // The AC combination: q search over a detail payload that the
        // log-compression job has zstd-compressed (the raw `detail` column
        // is blanked, so only the decompressing matcher can find it).
        let (_tmp, s) = setup();
        let mut old = make_event("e-old", "a1", AuditEventType::FileAccess, 100);
        old.detail = serde_json::json!({ "secret_name": "compressed_needle_secret" });
        old.tenant_id = Some("tenant-1".to_string());
        let mut new = make_event("e-new", "a2", AuditEventType::FileAccess, 200);
        new.detail = serde_json::json!({ "secret_name": "uncompressed_other" });
        new.tenant_id = Some("tenant-1".to_string());
        record(&s, &old).await;
        record(&s, &new).await;

        let stats = AuditRepository::compress_details_before(&s, 150, 10)
            .await
            .unwrap();
        assert_eq!(stats.compressed, 1, "the old event must be compressed");

        let filter = AuditQueryFilter {
            q: Some("compressed_needle".to_string()),
            ..all()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(
            results.len(),
            1,
            "compressed detail must still be searchable"
        );
        assert_eq!(results[0].id, Id::new("e-old"));
        assert_eq!(
            AuditRepository::count_filtered(&s, &filter).await.unwrap(),
            1
        );

        // Tenant scoping composes with the compressed-row q path: another
        // tenant's caller sees nothing (both rows carry explicit tenants,
        // so the NULL-legacy union does not apply here).
        let filter = AuditQueryFilter {
            q: Some("compressed_needle".to_string()),
            tenant_id: Some("tenant-other".to_string()),
            ..all()
        };
        assert_eq!(AuditRepository::query(&s, &filter).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn audit_free_text_search_pages_past_the_first_page() {
        // The q path fetches candidates in pages (page = max(limit,100));
        // with sparse matches the loop must keep paging to collect the
        // requested number of matches.
        let (_tmp, s) = setup();
        // 140 events; every 20th carries the needle → 7 matches scattered
        // across two pages of 100.
        for i in 0..140u64 {
            let mut e = make_event(
                &format!("e{}", i),
                "a1",
                AuditEventType::FileAccess,
                1000 + i,
            );
            if i % 20 == 0 {
                e.detail = serde_json::json!({ "needle": "sparse-needle-token" });
            } else {
                e.detail = serde_json::json!({ "filler": format!("filler-{i}") });
            }
            record(&s, &e).await;
        }

        let filter = AuditQueryFilter {
            q: Some("sparse-needle-token".to_string()),
            limit: 10,
            ..Default::default()
        };
        let results = AuditRepository::query(&s, &filter).await.unwrap();
        assert_eq!(results.len(), 7, "all sparse matches across pages");
        assert!(results
            .iter()
            .all(|e| e.detail["needle"] == "sparse-needle-token"));
        assert_eq!(
            AuditRepository::count_filtered(&s, &filter).await.unwrap(),
            7
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
