use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};
use gyre_ports::analytics::{AnalyticsRepository, CostRepository};

use super::{open_conn, SqliteStorage};

fn row_to_analytics_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsEvent> {
    let props_str: String = row.get(3)?;
    let properties: serde_json::Value =
        serde_json::from_str(&props_str).unwrap_or(serde_json::Value::Object(Default::default()));
    Ok(AnalyticsEvent {
        id: Id::new(row.get::<_, String>(0)?),
        event_name: row.get(1)?,
        agent_id: row.get(2)?,
        properties,
        timestamp: row.get::<_, i64>(4)? as u64,
    })
}

fn row_to_cost_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CostEntry> {
    Ok(CostEntry {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        task_id: row.get::<_, Option<String>>(2)?.map(Id::new),
        cost_type: row.get(3)?,
        amount: row.get(4)?,
        currency: row.get(5)?,
        timestamp: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl AnalyticsRepository for SqliteStorage {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        let path = self.db_path();
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let props = serde_json::to_string(&e.properties)?;
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO analytics_events (id, event_name, agent_id, properties, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    e.id.as_str(),
                    e.event_name,
                    e.agent_id,
                    props,
                    e.timestamp as i64,
                ],
            )
            .context("insert analytics_event")?;
            Ok(())
        })
        .await?
    }

    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>> {
        let path = self.db_path();
        let event_name = event_name.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<Vec<AnalyticsEvent>> {
            let conn = open_conn(&path)?;
            let mut conditions: Vec<String> = Vec::new();
            if since.is_some() {
                conditions.push("timestamp >= ?1".to_string());
            }
            if event_name.is_some() {
                conditions.push(format!("event_name = ?{}", conditions.len() + 1));
            }
            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };
            let sql = format!(
                "SELECT id, event_name, agent_id, properties, timestamp
                 FROM analytics_events {} ORDER BY timestamp DESC LIMIT {}",
                where_clause, limit
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            if let Some(s) = since {
                bind_values.push(Box::new(s as i64));
            }
            if let Some(ref name) = event_name {
                bind_values.push(Box::new(name.clone()));
            }
            let refs: Vec<&dyn rusqlite::types::ToSql> =
                bind_values.iter().map(|b| b.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), row_to_analytics_event)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64> {
        let path = self.db_path();
        let event_name = event_name.to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let conn = open_conn(&path)?;
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM analytics_events
                 WHERE event_name = ?1 AND timestamp >= ?2 AND timestamp <= ?3",
                rusqlite::params![event_name, since as i64, until as i64],
                |row| row.get(0),
            )?;
            Ok(n as u64)
        })
        .await?
    }

    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>> {
        let path = self.db_path();
        let event_name = event_name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT date(timestamp, 'unixepoch') as day, COUNT(*) as cnt
                 FROM analytics_events
                 WHERE event_name = ?1 AND timestamp >= ?2 AND timestamp <= ?3
                 GROUP BY day ORDER BY day",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![event_name, since as i64, until as i64],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64)),
            )?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }
}

#[async_trait]
impl CostRepository for SqliteStorage {
    async fn record(&self, entry: &CostEntry) -> Result<()> {
        let path = self.db_path();
        let e = entry.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO cost_entries (id, agent_id, task_id, cost_type, amount, currency, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    e.id.as_str(),
                    e.agent_id.as_str(),
                    e.task_id.as_ref().map(|id| id.as_str()),
                    e.cost_type,
                    e.amount,
                    e.currency,
                    e.timestamp as i64,
                ],
            )
            .context("insert cost_entry")?;
            Ok(())
        })
        .await?
    }

    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<CostEntry>> {
            let conn = open_conn(&path)?;
            let (sql, use_since) = if since.is_some() {
                (
                    "SELECT id, agent_id, task_id, cost_type, amount, currency, timestamp
                     FROM cost_entries WHERE agent_id = ?1 AND timestamp >= ?2
                     ORDER BY timestamp DESC",
                    true,
                )
            } else {
                (
                    "SELECT id, agent_id, task_id, cost_type, amount, currency, timestamp
                     FROM cost_entries WHERE agent_id = ?1
                     ORDER BY timestamp DESC",
                    false,
                )
            };
            let mut stmt = conn.prepare(sql)?;
            let rows = if use_since {
                stmt.query_map(
                    rusqlite::params![agent_id.as_str(), since.unwrap() as i64],
                    row_to_cost_entry,
                )?
                .collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map(rusqlite::params![agent_id.as_str()], row_to_cost_entry)?
                    .collect::<Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
        .await?
    }

    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>> {
        let path = self.db_path();
        let task_id = task_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<CostEntry>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, task_id, cost_type, amount, currency, timestamp
                 FROM cost_entries WHERE task_id = ?1 ORDER BY timestamp DESC",
            )?;
            let rows = stmt.query_map(rusqlite::params![task_id.as_str()], row_to_cost_entry)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<f64> {
            let conn = open_conn(&path)?;
            let total: f64 = conn.query_row(
                "SELECT COALESCE(SUM(amount), 0.0) FROM cost_entries WHERE agent_id = ?1",
                rusqlite::params![agent_id.as_str()],
                |row| row.get(0),
            )?;
            Ok(total)
        })
        .await?
    }

    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<f64> {
            let conn = open_conn(&path)?;
            let total: f64 = conn.query_row(
                "SELECT COALESCE(SUM(amount), 0.0) FROM cost_entries
                 WHERE timestamp >= ?1 AND timestamp <= ?2",
                rusqlite::params![since as i64, until as i64],
                |row| row.get(0),
            )?;
            Ok(total)
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

    fn make_event(id: &str, name: &str, ts: u64) -> AnalyticsEvent {
        AnalyticsEvent::new(
            Id::new(id),
            name,
            Some("agent-1".to_string()),
            serde_json::json!({ "key": "val" }),
            ts,
        )
    }

    fn make_cost(
        id: &str,
        agent_id: &str,
        task_id: Option<&str>,
        amount: f64,
        ts: u64,
    ) -> CostEntry {
        CostEntry::new(
            Id::new(id),
            Id::new(agent_id),
            task_id.map(|t| Id::new(t)),
            "llm_tokens",
            amount,
            "tokens",
            ts,
        )
    }

    // --- AnalyticsRepository tests ---

    #[tokio::test]
    async fn analytics_record_and_query_all() {
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("e1", "task.completed", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "mr.merged", 200))
            .await
            .unwrap();
        let results = AnalyticsRepository::query(&s, None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn analytics_query_by_event_name() {
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("e1", "task.completed", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "mr.merged", 200))
            .await
            .unwrap();
        let results = AnalyticsRepository::query(&s, Some("task.completed"), None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_name, "task.completed");
    }

    #[tokio::test]
    async fn analytics_query_with_since() {
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("e1", "ev", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "ev", 200))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e3", "ev", 300))
            .await
            .unwrap();
        let results = AnalyticsRepository::query(&s, None, Some(150), 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.timestamp >= 150));
    }

    #[tokio::test]
    async fn analytics_query_limit() {
        let (_tmp, s) = setup();
        for i in 0..5u64 {
            AnalyticsRepository::record(&s, &make_event(&format!("e{}", i), "ev", i * 100))
                .await
                .unwrap();
        }
        let results = AnalyticsRepository::query(&s, None, None, 3).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn analytics_count() {
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("e1", "task.completed", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "task.completed", 200))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e3", "mr.merged", 150))
            .await
            .unwrap();
        let count = AnalyticsRepository::count(&s, "task.completed", 0, 9999)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn analytics_aggregate_by_day() {
        let (_tmp, s) = setup();
        // Jan 1 2024 00:00:00 UTC = 1704067200
        let day1: u64 = 1704067200;
        let day2: u64 = 1704067200 + 86400;
        AnalyticsRepository::record(&s, &make_event("e1", "ev", day1))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "ev", day1 + 3600))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e3", "ev", day2))
            .await
            .unwrap();
        let agg = AnalyticsRepository::aggregate_by_day(&s, "ev", 0, day2 + 86400)
            .await
            .unwrap();
        assert_eq!(agg.len(), 2);
        let d1 = agg.iter().find(|(d, _)| d == "2024-01-01").unwrap();
        assert_eq!(d1.1, 2);
        let d2 = agg.iter().find(|(d, _)| d == "2024-01-02").unwrap();
        assert_eq!(d2.1, 1);
    }

    // --- CostRepository tests ---

    #[tokio::test]
    async fn cost_record_and_query_by_agent() {
        let (_tmp, s) = setup();
        CostRepository::record(&s, &make_cost("c1", "agent-1", None, 100.0, 1000))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c2", "agent-2", None, 50.0, 2000))
            .await
            .unwrap();
        let results = CostRepository::query_by_agent(&s, &Id::new("agent-1"), None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].amount, 100.0);
    }

    #[tokio::test]
    async fn cost_query_by_agent_with_since() {
        let (_tmp, s) = setup();
        CostRepository::record(&s, &make_cost("c1", "agent-1", None, 100.0, 100))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c2", "agent-1", None, 200.0, 500))
            .await
            .unwrap();
        let results = CostRepository::query_by_agent(&s, &Id::new("agent-1"), Some(300))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].amount, 200.0);
    }

    #[tokio::test]
    async fn cost_query_by_task() {
        let (_tmp, s) = setup();
        CostRepository::record(&s, &make_cost("c1", "agent-1", Some("task-1"), 100.0, 1000))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c2", "agent-1", Some("task-2"), 50.0, 2000))
            .await
            .unwrap();
        let results = CostRepository::query_by_task(&s, &Id::new("task-1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].amount, 100.0);
    }

    #[tokio::test]
    async fn cost_total_by_agent() {
        let (_tmp, s) = setup();
        CostRepository::record(&s, &make_cost("c1", "agent-1", None, 100.0, 1000))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c2", "agent-1", None, 250.0, 2000))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c3", "agent-2", None, 999.0, 3000))
            .await
            .unwrap();
        let total = CostRepository::total_by_agent(&s, &Id::new("agent-1"))
            .await
            .unwrap();
        assert!((total - 350.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn cost_total_by_period() {
        let (_tmp, s) = setup();
        CostRepository::record(&s, &make_cost("c1", "agent-1", None, 100.0, 100))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c2", "agent-1", None, 200.0, 500))
            .await
            .unwrap();
        CostRepository::record(&s, &make_cost("c3", "agent-2", None, 50.0, 1000))
            .await
            .unwrap();
        let total = CostRepository::total_by_period(&s, 200, 600).await.unwrap();
        assert!((total - 200.0).abs() < 0.001);
    }
}
