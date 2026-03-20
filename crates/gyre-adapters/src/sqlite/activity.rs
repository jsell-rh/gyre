use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::ActivityEvent;
use gyre_ports::activity::{ActivityQuery, ActivityRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::activity_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = activity_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ActivityEventRow {
    id: String,
    agent_id: String,
    event_type: String,
    description: String,
    timestamp: i64,
    #[allow(dead_code)]
    tenant_id: String,
}

impl From<ActivityEventRow> for ActivityEvent {
    fn from(r: ActivityEventRow) -> Self {
        ActivityEvent {
            id: Id::new(r.id),
            agent_id: r.agent_id,
            event_type: r.event_type,
            description: r.description,
            timestamp: r.timestamp as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = activity_events)]
struct ActivityEventRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    event_type: &'a str,
    description: &'a str,
    timestamp: i64,
    tenant_id: &'a str,
}

impl<'a> From<&'a ActivityEvent> for ActivityEventRecord<'a> {
    fn from(e: &'a ActivityEvent) -> Self {
        ActivityEventRecord {
            id: e.id.as_str(),
            agent_id: &e.agent_id,
            event_type: &e.event_type,
            description: &e.description,
            timestamp: e.timestamp as i64,
            tenant_id: "default",
        }
    }
}

#[async_trait]
impl ActivityRepository for SqliteStorage {
    async fn append(&self, event: &ActivityEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = ActivityEventRecord::from(&e);
            diesel::insert_into(activity_events::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert activity_event")?;
            Ok(())
        })
        .await?
    }

    async fn query(&self, q: &ActivityQuery) -> Result<Vec<ActivityEvent>> {
        let pool = Arc::clone(&self.pool);
        let since = q.since;
        let limit = q.limit;
        let agent_id = q.agent_id.clone();
        let event_type = q.event_type.clone();

        tokio::task::spawn_blocking(move || -> Result<Vec<ActivityEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = activity_events::table
                .order(activity_events::timestamp.asc())
                .into_boxed();
            if let Some(s) = since {
                query = query.filter(activity_events::timestamp.ge(s as i64));
            }
            if let Some(ref a) = agent_id {
                query = query.filter(activity_events::agent_id.eq(a.as_str()));
            }
            if let Some(ref et) = event_type {
                query = query.filter(activity_events::event_type.eq(et.as_str()));
            }
            if let Some(l) = limit {
                query = query.limit(l as i64);
            }
            let rows = query
                .load::<ActivityEventRow>(&mut *conn)
                .context("query activity_events")?;
            Ok(rows.into_iter().map(ActivityEvent::from).collect())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_event(id: &str, agent_id: &str, event_type: &str, ts: u64) -> ActivityEvent {
        ActivityEvent::new(
            Id::new(id),
            agent_id,
            event_type,
            format!("event {}", id),
            ts,
        )
    }

    #[tokio::test]
    async fn append_and_query_all() {
        let (_tmp, s) = setup();
        s.append(&make_event("e1", "agent-a", "task_started", 100))
            .await
            .unwrap();
        s.append(&make_event("e2", "agent-b", "task_done", 200))
            .await
            .unwrap();

        let q = ActivityQuery {
            since: None,
            limit: None,
            agent_id: None,
            event_type: None,
        };
        let results = s.query(&q).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn query_with_since() {
        let (_tmp, s) = setup();
        s.append(&make_event("e1", "a", "t", 100)).await.unwrap();
        s.append(&make_event("e2", "a", "t", 200)).await.unwrap();
        s.append(&make_event("e3", "a", "t", 300)).await.unwrap();

        let q = ActivityQuery {
            since: Some(150),
            limit: None,
            agent_id: None,
            event_type: None,
        };
        let results = s.query(&q).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.timestamp >= 150));
    }

    #[tokio::test]
    async fn query_with_limit() {
        let (_tmp, s) = setup();
        for i in 0..5u64 {
            s.append(&make_event(&format!("e{}", i), "a", "t", i * 100))
                .await
                .unwrap();
        }
        let q = ActivityQuery {
            since: None,
            limit: Some(3),
            agent_id: None,
            event_type: None,
        };
        let results = s.query(&q).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn query_by_agent() {
        let (_tmp, s) = setup();
        s.append(&make_event("e1", "agent-a", "t", 100))
            .await
            .unwrap();
        s.append(&make_event("e2", "agent-b", "t", 200))
            .await
            .unwrap();
        s.append(&make_event("e3", "agent-a", "t", 300))
            .await
            .unwrap();

        let q = ActivityQuery {
            since: None,
            limit: None,
            agent_id: Some("agent-a".to_string()),
            event_type: None,
        };
        let results = s.query(&q).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.agent_id == "agent-a"));
    }

    #[tokio::test]
    async fn query_by_event_type() {
        let (_tmp, s) = setup();
        s.append(&make_event("e1", "a", "task_started", 100))
            .await
            .unwrap();
        s.append(&make_event("e2", "a", "task_done", 200))
            .await
            .unwrap();
        s.append(&make_event("e3", "b", "task_started", 300))
            .await
            .unwrap();

        let q = ActivityQuery {
            since: None,
            limit: None,
            agent_id: None,
            event_type: Some("task_started".to_string()),
        };
        let results = s.query(&q).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.event_type == "task_started"));
    }

    #[tokio::test]
    async fn query_empty_returns_empty() {
        let (_tmp, s) = setup();
        let q = ActivityQuery {
            since: None,
            limit: None,
            agent_id: None,
            event_type: None,
        };
        assert!(s.query(&q).await.unwrap().is_empty());
    }
}
