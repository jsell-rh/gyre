use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
use gyre_ports::{AuditQueryFilter, AuditRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::audit_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
    AuditEventRecord {
        id: e.id.as_str(),
        event_type: e.event_type.as_str(),
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
impl AuditRepository for SqliteStorage {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
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
            let rows = apply_filters(audit_events::table.into_boxed(), &filter)
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

type BoxedAuditQuery = diesel::dsl::IntoBoxed<
    'static,
    crate::schema::audit_events::table,
    diesel::sqlite::Sqlite,
>;

fn apply_filters(
    mut query: BoxedAuditQuery,
    filter: &AuditQueryFilter,
) -> BoxedAuditQuery {
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
    if let Some(u) = &filter.user_id {
        query = query.filter(audit_events::user_id.eq(u.clone()));
    }
    if let Some(rt) = &filter.resource_type {
        query = query.filter(audit_events::resource_type.eq(rt.clone()));
    }
    if let Some(o) = filter.outcome {
        query = query.filter(audit_events::outcome.eq(o.as_str()));
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
        record(&s, &make_event("e1", "agent-1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200)).await;
        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn audit_query_by_agent() {
        let (_tmp, s) = setup();
        record(&s, &make_event("e1", "agent-1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "agent-2", AuditEventType::ProcessExec, 200)).await;
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
        record(&s, &make_event("e1", "agent-1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200)).await;
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
                &make_event(&format!("e{}", i), "agent-1", AuditEventType::Syscall, i * 100),
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
    async fn audit_count() {
        let (_tmp, s) = setup();
        record(&s, &make_event("e1", "agent-1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "agent-1", AuditEventType::Syscall, 200)).await;
        let count = AuditRepository::count(&s).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn audit_stats_by_type() {
        let (_tmp, s) = setup();
        record(&s, &make_event("e1", "a1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("e2", "a1", AuditEventType::FileAccess, 200)).await;
        record(&s, &make_event("e3", "a1", AuditEventType::NetworkConnect, 300)).await;
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
        record(&s, &make_event("old", "agent-1", AuditEventType::FileAccess, 100)).await;
        record(&s, &make_event("new", "agent-1", AuditEventType::FileAccess, 200)).await;

        let deleted = AuditRepository::delete_older_than(&s, 150).await.unwrap();
        assert_eq!(deleted, 1);

        let results = AuditRepository::query(&s, &all()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("new"));

        // Second run is idempotent.
        assert_eq!(AuditRepository::delete_older_than(&s, 150).await.unwrap(), 0);
    }
}
