use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_ports::{AuditQueryFilter, AuditRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::audit_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct AuditEventRow {
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
    source_ip: Option<String>,
    user_agent: Option<String>,
    timestamp: i64,
}

impl From<AuditEventRow> for AuditEvent {
    fn from(r: AuditEventRow) -> Self {
        let detail: serde_json::Value = serde_json::from_str(&r.detail)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        AuditEvent {
            id: Id::new(r.id),
            event_type: AuditEventType::from_str(&r.event_type),
            agent_id: r.agent_id.map(Id::new),
            user_id: r.user_id.map(Id::new),
            session_id: r.session_id,
            workspace_id: r.workspace_id.map(Id::new),
            repo_id: r.repo_id.map(Id::new),
            resource_type: r.resource_type,
            resource_id: r.resource_id,
            outcome: AuditOutcome::from_str(&r.outcome).unwrap_or(AuditOutcome::Success),
            detail,
            source_ip: r.source_ip,
            user_agent: r.user_agent,
            timestamp: r.timestamp as u64,
        }
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
}
