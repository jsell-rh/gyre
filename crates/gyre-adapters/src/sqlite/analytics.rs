use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};
use gyre_ports::analytics::{AnalyticsRepository, CostRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{analytics_events, cost_entries};

#[derive(Queryable, Selectable)]
#[diesel(table_name = analytics_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AnalyticsEventRow {
    id: String,
    event_name: String,
    agent_id: Option<String>,
    properties: String,
    timestamp: i64,
    #[allow(dead_code)]
    tenant_id: String,
}

impl From<AnalyticsEventRow> for AnalyticsEvent {
    fn from(r: AnalyticsEventRow) -> Self {
        let properties: serde_json::Value = serde_json::from_str(&r.properties)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        AnalyticsEvent {
            id: Id::new(r.id),
            event_name: r.event_name,
            agent_id: r.agent_id,
            properties,
            timestamp: r.timestamp as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = analytics_events)]
struct AnalyticsEventRecord<'a> {
    id: &'a str,
    event_name: &'a str,
    agent_id: Option<&'a str>,
    properties: String,
    timestamp: i64,
    tenant_id: &'a str,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = cost_entries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct CostEntryRow {
    id: String,
    agent_id: String,
    task_id: Option<String>,
    cost_type: String,
    amount: f64,
    currency: String,
    timestamp: i64,
    #[allow(dead_code)]
    tenant_id: String,
}

impl From<CostEntryRow> for CostEntry {
    fn from(r: CostEntryRow) -> Self {
        CostEntry {
            id: Id::new(r.id),
            agent_id: Id::new(r.agent_id),
            task_id: r.task_id.map(Id::new),
            cost_type: r.cost_type,
            amount: r.amount,
            currency: r.currency,
            timestamp: r.timestamp as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = cost_entries)]
struct CostEntryRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    task_id: Option<&'a str>,
    cost_type: &'a str,
    amount: f64,
    currency: &'a str,
    timestamp: i64,
    tenant_id: &'a str,
}

#[derive(QueryableByName)]
struct DayCount {
    #[diesel(sql_type = Text)]
    day: String,
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

#[async_trait]
impl AnalyticsRepository for SqliteStorage {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let props = serde_json::to_string(&e.properties)?;
            let record = AnalyticsEventRecord {
                id: e.id.as_str(),
                event_name: &e.event_name,
                agent_id: e.agent_id.as_deref(),
                properties: props,
                timestamp: e.timestamp as i64,
                tenant_id: "default",
            };
            diesel::insert_into(analytics_events::table)
                .values(&record)
                .execute(&mut *conn)
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
        let pool = Arc::clone(&self.pool);
        let event_name = event_name.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<Vec<AnalyticsEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = analytics_events::table.into_boxed();
            if let Some(s) = since {
                query = query.filter(analytics_events::timestamp.ge(s as i64));
            }
            if let Some(ref name) = event_name {
                query = query.filter(analytics_events::event_name.eq(name.as_str()));
            }
            let rows = query
                .order(analytics_events::timestamp.desc())
                .limit(limit as i64)
                .load::<AnalyticsEventRow>(&mut *conn)
                .context("query analytics_events")?;
            Ok(rows.into_iter().map(AnalyticsEvent::from).collect())
        })
        .await?
    }

    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let event_name = event_name.to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = analytics_events::table
                .filter(analytics_events::event_name.eq(event_name.as_str()))
                .filter(analytics_events::timestamp.ge(since as i64))
                .filter(analytics_events::timestamp.le(until as i64))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count analytics_events")?;
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
        let pool = Arc::clone(&self.pool);
        let event_name = event_name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = diesel::sql_query(
                "SELECT date(timestamp, 'unixepoch') as day, COUNT(*) as cnt \
                 FROM analytics_events \
                 WHERE event_name = ? AND timestamp >= ? AND timestamp <= ? \
                 GROUP BY day ORDER BY day",
            )
            .bind::<Text, _>(event_name)
            .bind::<BigInt, _>(since as i64)
            .bind::<BigInt, _>(until as i64)
            .load::<DayCount>(&mut *conn)
            .context("aggregate analytics_events by day")?;
            Ok(rows.into_iter().map(|r| (r.day, r.cnt as u64)).collect())
        })
        .await?
    }
}

#[async_trait]
impl CostRepository for SqliteStorage {
    async fn record(&self, entry: &CostEntry) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = entry.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = CostEntryRecord {
                id: e.id.as_str(),
                agent_id: e.agent_id.as_str(),
                task_id: e.task_id.as_ref().map(|id| id.as_str()),
                cost_type: &e.cost_type,
                amount: e.amount,
                currency: &e.currency,
                timestamp: e.timestamp as i64,
                tenant_id: "default",
            };
            diesel::insert_into(cost_entries::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert cost_entry")?;
            Ok(())
        })
        .await?
    }

    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<CostEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = cost_entries::table
                .filter(cost_entries::agent_id.eq(agent_id.as_str()))
                .order(cost_entries::timestamp.desc())
                .into_boxed();
            if let Some(s) = since {
                query = query.filter(cost_entries::timestamp.ge(s as i64));
            }
            let rows = query
                .load::<CostEntryRow>(&mut *conn)
                .context("query cost_entries by agent")?;
            Ok(rows.into_iter().map(CostEntry::from).collect())
        })
        .await?
    }

    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>> {
        let pool = Arc::clone(&self.pool);
        let task_id = task_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<CostEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = cost_entries::table
                .filter(cost_entries::task_id.eq(task_id.as_str()))
                .order(cost_entries::timestamp.desc())
                .load::<CostEntryRow>(&mut *conn)
                .context("query cost_entries by task")?;
            Ok(rows.into_iter().map(CostEntry::from).collect())
        })
        .await?
    }

    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<f64> {
            let mut conn = pool.get().context("get db connection")?;
            let total = cost_entries::table
                .filter(cost_entries::agent_id.eq(agent_id.as_str()))
                .select(diesel::dsl::sum(cost_entries::amount))
                .get_result::<Option<f64>>(&mut *conn)
                .context("total cost by agent")?;
            Ok(total.unwrap_or(0.0))
        })
        .await?
    }

    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<f64> {
            let mut conn = pool.get().context("get db connection")?;
            let total = cost_entries::table
                .filter(cost_entries::timestamp.ge(since as i64))
                .filter(cost_entries::timestamp.le(until as i64))
                .select(diesel::dsl::sum(cost_entries::amount))
                .get_result::<Option<f64>>(&mut *conn)
                .context("total cost by period")?;
            Ok(total.unwrap_or(0.0))
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
            task_id.map(Id::new),
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
