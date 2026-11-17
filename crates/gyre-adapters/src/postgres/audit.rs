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
        let e = event.clone();
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
            let mut query = audit_events::table.into_boxed();
            if let Some(s) = filter.since {
                query = query.filter(audit_events::timestamp.ge(s as i64));
            }
            if let Some(u) = filter.until {
                query = query.filter(audit_events::timestamp.le(u as i64));
            }
            if let Some(a) = &filter.agent_id {
                query = query.filter(audit_events::agent_id.eq(a.as_str()));
            }
            if let Some(et) = &filter.event_type {
                query = query.filter(audit_events::event_type.eq(et.as_str()));
            }
            if let Some(w) = &filter.workspace_id {
                query = query.filter(audit_events::workspace_id.eq(w.as_str()));
            }
            if let Some(u) = &filter.user_id {
                query = query.filter(audit_events::user_id.eq(u.as_str()));
            }
            if let Some(rt) = &filter.resource_type {
                query = query.filter(audit_events::resource_type.eq(rt.as_str()));
            }
            if let Some(o) = filter.outcome {
                query = query.filter(audit_events::outcome.eq(o.as_str()));
            }
            let rows = query
                .order(audit_events::timestamp.desc())
                .limit(filter.limit as i64)
                .load::<AuditEventRow>(&mut *conn)
                .context("query audit_events")?;
            rows.into_iter()
                .map(AuditEventRow::into_event)
                .collect::<Result<Vec<_>>>()
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
