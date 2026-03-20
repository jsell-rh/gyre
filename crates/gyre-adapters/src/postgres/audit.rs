use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType};
use gyre_ports::AuditRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::audit_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct AuditEventRow {
    id: String,
    agent_id: String,
    event_type: String,
    path: Option<String>,
    details: String,
    pid: Option<i32>,
    timestamp: i64,
}

impl From<AuditEventRow> for AuditEvent {
    fn from(r: AuditEventRow) -> Self {
        let details: serde_json::Value = serde_json::from_str(&r.details)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        AuditEvent {
            id: Id::new(r.id),
            agent_id: Id::new(r.agent_id),
            event_type: AuditEventType::from_str(&r.event_type),
            path: r.path,
            details,
            pid: r.pid.map(|v| v as u32),
            timestamp: r.timestamp as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = audit_events)]
struct AuditEventRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    event_type: &'a str,
    path: Option<&'a str>,
    details: String,
    pid: Option<i32>,
    timestamp: i64,
}

#[derive(QueryableByName)]
struct EventTypeStat {
    #[diesel(sql_type = Text)]
    event_type: String,
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

#[async_trait]
impl AuditRepository for PgStorage {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let details = serde_json::to_string(&e.details)?;
            let event_type_str = e.event_type.as_str().to_string();
            let record = AuditEventRecord {
                id: e.id.as_str(),
                agent_id: e.agent_id.as_str(),
                event_type: &event_type_str,
                path: e.path.as_deref(),
                details,
                pid: e.pid.map(|p| p as i32),
                timestamp: e.timestamp as i64,
            };
            diesel::insert_into(audit_events::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert audit_event")?;
            Ok(())
        })
        .await?
    }

    async fn query(
        &self,
        agent_id: Option<&str>,
        event_type: Option<&str>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.map(|s| s.to_string());
        let event_type = event_type.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = audit_events::table.into_boxed();
            if let Some(s) = since {
                query = query.filter(audit_events::timestamp.ge(s as i64));
            }
            if let Some(u) = until {
                query = query.filter(audit_events::timestamp.le(u as i64));
            }
            if let Some(ref a) = agent_id {
                query = query.filter(audit_events::agent_id.eq(a.as_str()));
            }
            if let Some(ref et) = event_type {
                query = query.filter(audit_events::event_type.eq(et.as_str()));
            }
            let rows = query
                .order(audit_events::timestamp.desc())
                .limit(limit as i64)
                .load::<AuditEventRow>(&mut *conn)
                .context("query audit_events")?;
            Ok(rows.into_iter().map(AuditEvent::from).collect())
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
            Ok(rows.into_iter().map(AuditEvent::from).collect())
        })
        .await?
    }
}
