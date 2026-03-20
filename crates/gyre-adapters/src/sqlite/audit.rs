use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType};
use gyre_ports::AuditRepository;

use super::{open_conn, SqliteStorage};

fn row_to_audit_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEvent> {
    let details_str: String = row.get(4)?;
    let details: serde_json::Value =
        serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Object(Default::default()));
    let event_type_str: String = row.get(2)?;
    Ok(AuditEvent {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        event_type: AuditEventType::from_str(&event_type_str),
        path: row.get(3)?,
        details,
        pid: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        timestamp: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl AuditRepository for SqliteStorage {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let path = self.db_path();
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let details = serde_json::to_string(&e.details)?;
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO audit_events (id, agent_id, event_type, path, details, pid, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    e.id.as_str(),
                    e.agent_id.as_str(),
                    e.event_type.as_str(),
                    e.path,
                    details,
                    e.pid.map(|p| p as i64),
                    e.timestamp as i64,
                ],
            )
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
        let path = self.db_path();
        let agent_id = agent_id.map(|s| s.to_string());
        let event_type = event_type.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEvent>> {
            let conn = open_conn(&path)?;
            let mut conditions: Vec<String> = Vec::new();
            if since.is_some() {
                conditions.push(format!("timestamp >= ?{}", conditions.len() + 1));
            }
            if until.is_some() {
                conditions.push(format!("timestamp <= ?{}", conditions.len() + 1));
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
            let sql = format!(
                "SELECT id, agent_id, event_type, path, details, pid, timestamp
                 FROM audit_events {} ORDER BY timestamp DESC LIMIT {}",
                where_clause, limit
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            if let Some(s) = since {
                bind_values.push(Box::new(s as i64));
            }
            if let Some(u) = until {
                bind_values.push(Box::new(u as i64));
            }
            if let Some(ref a) = agent_id {
                bind_values.push(Box::new(a.clone()));
            }
            if let Some(ref et) = event_type {
                bind_values.push(Box::new(et.clone()));
            }
            let refs: Vec<&dyn rusqlite::types::ToSql> =
                bind_values.iter().map(|b| b.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), row_to_audit_event)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn count(&self) -> Result<u64> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let conn = open_conn(&path)?;
            let n: i64 =
                conn.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
            Ok(n as u64)
        })
        .await?
    }

    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT event_type, COUNT(*) as cnt FROM audit_events GROUP BY event_type ORDER BY cnt DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEvent>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(&format!(
                "SELECT id, agent_id, event_type, path, details, pid, timestamp
                 FROM audit_events WHERE timestamp > ?1
                 ORDER BY timestamp ASC LIMIT {}",
                limit
            ))?;
            let rows = stmt.query_map(rusqlite::params![since as i64], row_to_audit_event)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
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
            Id::new(agent),
            et,
            Some("/tmp/test".to_string()),
            serde_json::json!({ "action": "read" }),
            Some(1000),
            ts,
        )
    }

    #[tokio::test]
    async fn audit_record_and_query_all() {
        let (_tmp, s) = setup();
        AuditRepository::record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await
        .unwrap();
        AuditRepository::record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200),
        )
        .await
        .unwrap();
        let results = AuditRepository::query(&s, None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn audit_query_by_agent() {
        let (_tmp, s) = setup();
        AuditRepository::record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await
        .unwrap();
        AuditRepository::record(
            &s,
            &make_event("e2", "agent-2", AuditEventType::ProcessExec, 200),
        )
        .await
        .unwrap();
        let results = AuditRepository::query(&s, Some("agent-1"), None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id.as_str(), "agent-1");
    }

    #[tokio::test]
    async fn audit_query_by_event_type() {
        let (_tmp, s) = setup();
        AuditRepository::record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await
        .unwrap();
        AuditRepository::record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::NetworkConnect, 200),
        )
        .await
        .unwrap();
        let results = AuditRepository::query(&s, None, Some("file_access"), None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, AuditEventType::FileAccess);
    }

    #[tokio::test]
    async fn audit_query_since_until() {
        let (_tmp, s) = setup();
        for i in 1u64..=5 {
            AuditRepository::record(
                &s,
                &make_event(
                    &format!("e{}", i),
                    "agent-1",
                    AuditEventType::Syscall,
                    i * 100,
                ),
            )
            .await
            .unwrap();
        }
        let results = AuditRepository::query(&s, None, None, Some(200), Some(400), 100)
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
        AuditRepository::record(
            &s,
            &make_event("e1", "agent-1", AuditEventType::FileAccess, 100),
        )
        .await
        .unwrap();
        AuditRepository::record(
            &s,
            &make_event("e2", "agent-1", AuditEventType::Syscall, 200),
        )
        .await
        .unwrap();
        let count = AuditRepository::count(&s).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn audit_stats_by_type() {
        let (_tmp, s) = setup();
        AuditRepository::record(&s, &make_event("e1", "a1", AuditEventType::FileAccess, 100))
            .await
            .unwrap();
        AuditRepository::record(&s, &make_event("e2", "a1", AuditEventType::FileAccess, 200))
            .await
            .unwrap();
        AuditRepository::record(
            &s,
            &make_event("e3", "a1", AuditEventType::NetworkConnect, 300),
        )
        .await
        .unwrap();
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
            AuditRepository::record(
                &s,
                &make_event(&format!("e{}", i), "a1", AuditEventType::Syscall, i * 100),
            )
            .await
            .unwrap();
        }
        let results = AuditRepository::since_timestamp(&s, 300, 10).await.unwrap();
        assert_eq!(results.len(), 2); // timestamps 400, 500
        assert!(results.iter().all(|e| e.timestamp > 300));
    }

    #[tokio::test]
    async fn audit_custom_event_type() {
        let (_tmp, s) = setup();
        AuditRepository::record(
            &s,
            &make_event(
                "e1",
                "a1",
                AuditEventType::Custom("container_escape".to_string()),
                100,
            ),
        )
        .await
        .unwrap();
        let results = AuditRepository::query(&s, None, Some("container_escape"), None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].event_type,
            AuditEventType::Custom("container_escape".to_string())
        );
    }
}
