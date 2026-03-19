use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::ActivityEvent;
use gyre_ports::activity::{ActivityQuery, ActivityRepository};

use super::{open_conn, SqliteStorage};

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityEvent> {
    Ok(ActivityEvent {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: row.get(1)?,
        event_type: row.get(2)?,
        description: row.get(3)?,
        timestamp: row.get::<_, i64>(4)? as u64,
    })
}

#[async_trait]
impl ActivityRepository for SqliteStorage {
    async fn append(&self, event: &ActivityEvent) -> Result<()> {
        let path = self.db_path();
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO activity_events (id, agent_id, event_type, description, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    e.id.as_str(),
                    e.agent_id,
                    e.event_type,
                    e.description,
                    e.timestamp as i64,
                ],
            )
            .context("insert activity_event")?;
            Ok(())
        })
        .await?
    }

    async fn query(&self, q: &ActivityQuery) -> Result<Vec<ActivityEvent>> {
        let path = self.db_path();
        let since = q.since;
        let limit = q.limit;
        let agent_id = q.agent_id.clone();
        let event_type = q.event_type.clone();

        tokio::task::spawn_blocking(move || -> Result<Vec<ActivityEvent>> {
            let conn = open_conn(&path)?;

            // Build query dynamically based on filters
            let mut conditions: Vec<String> = Vec::new();
            if since.is_some() {
                conditions.push("timestamp >= ?1".to_string());
            }
            if agent_id.is_some() {
                conditions.push(format!("agent_id = ?{}", conditions.len() + 1));
            }
            if event_type.is_some() {
                conditions.push(format!("event_type = ?{}", conditions.len() + 1));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let limit_clause = limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default();

            let sql = format!(
                "SELECT id, agent_id, event_type, description, timestamp
                 FROM activity_events {} ORDER BY timestamp {}",
                where_clause, limit_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Bind params in order
            let mut param_idx = 1usize;
            let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(s) = since {
                bind_values.push(Box::new(s as i64));
                param_idx += 1;
            }
            if let Some(ref a) = agent_id {
                bind_values.push(Box::new(a.clone()));
                param_idx += 1;
            }
            if let Some(ref et) = event_type {
                bind_values.push(Box::new(et.clone()));
                let _ = param_idx;
            }

            let refs: Vec<&dyn rusqlite::types::ToSql> =
                bind_values.iter().map(|b| b.as_ref()).collect();

            let rows = stmt.query_map(refs.as_slice(), row_to_event)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
