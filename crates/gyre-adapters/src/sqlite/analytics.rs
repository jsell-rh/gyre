// `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
// `Self { field: field, … }` with spans mapped onto the field declarations
// below (DayCount, GroupCount), so clippy::redundant_field_names fires on
// derive-generated code. The generated construction sits in a dummy module
// that inherits neither struct- nor field-level `#[allow]` (verified: both
// still warn; same as postgres/search.rs), so the lint is scoped off for
// this whole module instead. Nothing in this file writes `field: field`
// initializations by hand.
#![allow(clippy::redundant_field_names)]
use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};
use gyre_ports::analytics::{
    AnalyticsGroupBy, AnalyticsQueryFilter, AnalyticsRepository, CostRepository,
};
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
    user_id: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    repo_id: Option<String>,
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
            user_id: r.user_id,
            session_id: r.session_id,
            workspace_id: r.workspace_id,
            repo_id: r.repo_id,
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
    user_id: Option<&'a str>,
    session_id: Option<&'a str>,
    workspace_id: Option<&'a str>,
    repo_id: Option<&'a str>,
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

// The QueryableByName derive expands to field-name-shorthand struct
// initialization, tripping clippy::redundant_field_names with spans on
// these definition lines (same as DayCount above); see the module-level
// allow at the top of this file.
#[derive(QueryableByName)]
struct GroupCount {
    #[diesel(sql_type = Text)]
    k: String,
    #[diesel(sql_type = BigInt)]
    cnt: i64,
}

#[async_trait]
impl AnalyticsRepository for SqliteStorage {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
        // File the row under the handle's resolved tenant scope (not a
        // fabricated literal) — same form as the merge_request/repository
        // create() rows (task-111 verification repair).
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let props = serde_json::to_string(&e.properties)?;
            let record = AnalyticsEventRecord {
                id: e.id.as_str(),
                event_name: &e.event_name,
                agent_id: e.agent_id.as_deref(),
                user_id: e.user_id.as_deref(),
                session_id: e.session_id.as_deref(),
                workspace_id: e.workspace_id.as_deref(),
                repo_id: e.repo_id.as_deref(),
                properties: props,
                timestamp: e.timestamp as i64,
                tenant_id: &tenant,
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

    async fn query_filtered(&self, filter: &AnalyticsQueryFilter) -> Result<Vec<AnalyticsEvent>> {
        let pool = Arc::clone(&self.pool);
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AnalyticsEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = analytics_events::table.into_boxed();
            if let Some(ref name) = filter.event_name {
                // Exact or trailing-`*` prefix match (analytics.md
                // §Query Parameters). Prefix matching is LITERAL and
                // case-sensitive (task-146 review R4): SQLite LIKE is
                // ASCII-case-insensitive and honors `%`/`_` wildcards, so
                // `MR.*` and `mr.m_rged*` would both match `mr.merged`,
                // diverging from the mem adapter's `starts_with` and
                // Postgres's case-sensitive LIKE. `substr(...) = ...`
                // compares under the column's BINARY collation — literal
                // bytes, no wildcards — matching both exactly.
                if let Some(prefix) = name.strip_suffix('*') {
                    // `n` is a Rust-computed character count formatted as
                    // a numeric SQL literal (injection-free by
                    // construction); the prefix itself is a bound
                    // parameter. diesel's `sql(...).bind(...)` appends
                    // its `?` AFTER the fragment, so the length cannot be
                    // a second placeholder.
                    let n = prefix.chars().count();
                    let predicate = format!("substr(event_name, 1, {n}) = ");
                    query = query.filter(
                        diesel::dsl::sql::<diesel::sql_types::Bool>(&predicate)
                            .bind::<Text, _>(prefix),
                    );
                } else {
                    query = query.filter(analytics_events::event_name.eq(name.as_str()));
                }
            }
            if let Some(ref id) = filter.agent_id {
                query = query.filter(analytics_events::agent_id.eq(id.as_str()));
            }
            if let Some(ref id) = filter.user_id {
                query = query.filter(analytics_events::user_id.eq(id.as_str()));
            }
            if let Some(ref id) = filter.workspace_id {
                query = query.filter(analytics_events::workspace_id.eq(id.as_str()));
            }
            if let Some(ref id) = filter.repo_id {
                query = query.filter(analytics_events::repo_id.eq(id.as_str()));
            }
            if let Some(s) = filter.since {
                query = query.filter(analytics_events::timestamp.ge(s as i64));
            }
            if let Some(u) = filter.until {
                query = query.filter(analytics_events::timestamp.le(u as i64));
            }
            let rows = query
                .order(analytics_events::timestamp.desc())
                .limit(filter.limit as i64)
                .load::<AnalyticsEventRow>(&mut *conn)
                .context("query analytics_events (filtered)")?;
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

    async fn count_grouped(
        &self,
        filter: &AnalyticsQueryFilter,
        group_by: AnalyticsGroupBy,
    ) -> Result<Vec<(String, u64)>> {
        let pool = Arc::clone(&self.pool);
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let mut conn = pool.get().context("get db connection")?;
            // Server-side aggregation over the FULL filtered set (task-146
            // review F1): the WHERE clause mirrors query_filtered exactly
            // (exact/trailing-`*` event_name, scope equality, inclusive
            // since/until); LIMIT bounds the number of returned groups.
            let key_expr = match group_by {
                AnalyticsGroupBy::EventName => "event_name".to_string(),
                AnalyticsGroupBy::AgentId => "COALESCE(agent_id, '(none)')".to_string(),
                AnalyticsGroupBy::WorkspaceId => "COALESCE(workspace_id, '(none)')".to_string(),
                AnalyticsGroupBy::Day => "date(timestamp, 'unixepoch')".to_string(),
            };
            let mut sql = format!(
                "SELECT {key_expr} AS k, COUNT(*) AS cnt \
                 FROM analytics_events WHERE 1=1"
            );
            if filter.event_name.is_some() {
                // Exact or trailing-`*` prefix match (analytics.md
                // §Query Parameters), same operator choice as
                // query_filtered: `=` for exact, literal-case-sensitive
                // `substr(...) = ...` for prefix (task-146 review R4 —
                // SQLite LIKE would be case-insensitive and honor `%`/`_`
                // wildcards, diverging from mem and Postgres).
                let is_prefix = filter
                    .event_name
                    .as_deref()
                    .is_some_and(|n| n.ends_with('*'));
                sql.push_str(if is_prefix {
                    " AND substr(event_name, 1, length(?)) = ?"
                } else {
                    " AND event_name = ?"
                });
            }
            if filter.agent_id.is_some() {
                sql.push_str(" AND agent_id = ?");
            }
            if filter.user_id.is_some() {
                sql.push_str(" AND user_id = ?");
            }
            if filter.workspace_id.is_some() {
                sql.push_str(" AND workspace_id = ?");
            }
            if filter.repo_id.is_some() {
                sql.push_str(" AND repo_id = ?");
            }
            if filter.since.is_some() {
                sql.push_str(" AND timestamp >= ?");
            }
            if filter.until.is_some() {
                sql.push_str(" AND timestamp <= ?");
            }
            sql.push_str(" GROUP BY k ORDER BY k LIMIT ?");
            // into_boxed(): bind() returns Self, so binds can be applied
            // conditionally (diesel's documented loop-binding pattern).
            let mut query = diesel::sql_query(&sql).into_boxed();
            if let Some(name) = &filter.event_name {
                match name.strip_suffix('*') {
                    Some(prefix) => {
                        // Two `?` placeholders in the substr prefix
                        // predicate — bind the literal prefix twice.
                        query = query.bind::<Text, _>(prefix);
                        query = query.bind::<Text, _>(prefix);
                    }
                    None => {
                        query = query.bind::<Text, _>(name);
                    }
                }
            }
            if let Some(id) = &filter.agent_id {
                query = query.bind::<Text, _>(id);
            }
            if let Some(id) = &filter.user_id {
                query = query.bind::<Text, _>(id);
            }
            if let Some(id) = &filter.workspace_id {
                query = query.bind::<Text, _>(id);
            }
            if let Some(id) = &filter.repo_id {
                query = query.bind::<Text, _>(id);
            }
            if let Some(s) = filter.since {
                query = query.bind::<BigInt, _>(s as i64);
            }
            if let Some(u) = filter.until {
                query = query.bind::<BigInt, _>(u as i64);
            }
            query = query.bind::<BigInt, _>(filter.limit as i64);
            let rows = query
                .load::<GroupCount>(&mut *conn)
                .context("count_grouped analytics_events")?;
            Ok(rows.into_iter().map(|r| (r.k, r.cnt as u64)).collect())
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

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = diesel::delete(analytics_events::table)
                .filter(analytics_events::timestamp.lt(cutoff_secs as i64))
                .execute(&mut *conn)
                .context("delete old analytics_events")?;
            Ok(n as u64)
        })
        .await?
    }
}

#[async_trait]
impl CostRepository for SqliteStorage {
    async fn record(&self, entry: &CostEntry) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = entry.clone();
        // Handle's resolved tenant scope — see AnalyticsRepository::record above.
        let tenant = self.tenant_id.clone();
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
                tenant_id: &tenant,
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

    #[tokio::test]
    async fn analytics_query_filtered_all_params() {
        let (_tmp, s) = setup();
        // Cross-cutting seed: agents span workspaces (a1 in ws-1 and ws-2),
        // workspaces span repos (ws-2 holds r1 and r2), and users span
        // workspaces (u1 in ws-1 and ws-3). No scope predicate is redundant
        // with another, so dropping any one predicate from query_filtered
        // must change the result sets asserted below.
        let mk = |id: &str,
                  name: &str,
                  agent: Option<&str>,
                  ws: Option<&str>,
                  repo: Option<&str>,
                  user: Option<&str>,
                  ts: u64| {
            AnalyticsEvent {
                id: Id::new(id),
                event_name: name.to_string(),
                agent_id: agent.map(str::to_string),
                user_id: user.map(str::to_string),
                session_id: None,
                workspace_id: ws.map(str::to_string),
                repo_id: repo.map(str::to_string),
                properties: serde_json::json!({}),
                timestamp: ts,
            }
        };
        AnalyticsRepository::record(
            &s,
            &mk(
                "f1",
                "mr.merged",
                Some("a1"),
                Some("ws-1"),
                Some("r1"),
                Some("u1"),
                100,
            ),
        )
        .await
        .unwrap();
        AnalyticsRepository::record(
            &s,
            &mk(
                "f2",
                "mr.merged",
                Some("a1"),
                Some("ws-2"),
                Some("r1"),
                None,
                200,
            ),
        )
        .await
        .unwrap();
        AnalyticsRepository::record(
            &s,
            &mk(
                "f3",
                "mr.merged",
                Some("a2"),
                Some("ws-2"),
                Some("r2"),
                Some("u2"),
                300,
            ),
        )
        .await
        .unwrap();
        AnalyticsRepository::record(
            &s,
            &mk(
                "f4",
                "mr.merged",
                Some("a3"),
                Some("ws-3"),
                Some("r3"),
                Some("u1"),
                400,
            ),
        )
        .await
        .unwrap();
        AnalyticsRepository::record(
            &s,
            &mk(
                "f5",
                "task.status_changed",
                Some("a4"),
                Some("ws-4"),
                Some("r4"),
                Some("u3"),
                500,
            ),
        )
        .await
        .unwrap();

        let ids = |out: &[AnalyticsEvent]| -> Vec<String> {
            let mut v: Vec<String> = out.iter().map(|e| e.id.as_str().to_string()).collect();
            v.sort();
            v
        };

        // Event name: exact.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.merged".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(
            ids(&out),
            vec!["f1", "f2", "f3", "f4"],
            "exact event_name match"
        );

        // Event name: prefix wildcard.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f1", "f2", "f3", "f4"], "mr.* prefix match");

        // Each scope predicate individually. Because the seed is
        // cross-cutting, each of these assertions fails if its predicate is
        // dropped from query_filtered (the unfiltered set is all 5 events).
        let f = AnalyticsQueryFilter {
            agent_id: Some("a1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f1", "f2"], "agent a1 spans ws-1 and ws-2");

        let f = AnalyticsQueryFilter {
            agent_id: Some("a2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f3"], "agent a2 only in ws-2");

        let f = AnalyticsQueryFilter {
            workspace_id: Some("ws-2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(
            ids(&out),
            vec!["f2", "f3"],
            "ws-2 holds two agents and two repos"
        );

        let f = AnalyticsQueryFilter {
            workspace_id: Some("ws-3".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f4"], "ws-3 holds only f4");

        let f = AnalyticsQueryFilter {
            repo_id: Some("r1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f1", "f2"], "repo r1 spans ws-1 and ws-2");

        let f = AnalyticsQueryFilter {
            repo_id: Some("r2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f3"], "repo r2 holds only f3");

        let f = AnalyticsQueryFilter {
            user_id: Some("u1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f1", "f4"], "user u1 spans ws-1 and ws-3");

        let f = AnalyticsQueryFilter {
            user_id: Some("u2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f3"], "user u2 holds only f3");

        // Conjunctive scope filters.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.merged".into()),
            agent_id: Some("a1".into()),
            workspace_id: Some("ws-1".into()),
            repo_id: Some("r1".into()),
            user_id: Some("u1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f1"], "all scope predicates conjunctive");

        // Inclusive since/until bounds.
        let f = AnalyticsQueryFilter {
            since: Some(200),
            until: Some(200),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(ids(&out), vec!["f2"], "inclusive since/until bounds");

        // Limit truncates, newest first.
        let f = AnalyticsQueryFilter {
            limit: 2,
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, Id::new("f5"), "newest event first");
        assert_eq!(out[1].id, Id::new("f4"), "second-newest next");
    }

    // --- count_grouped (task-146 review F1 regression) ---

    #[tokio::test]
    async fn analytics_count_grouped_over_full_filtered_set() {
        let (_tmp, s) = setup();
        // 150 events of one name — more than the filter's limit (100) —
        // plus a second name and a mixed agent/workspace population.
        let mk =
            |id: &str, name: &str, agent: Option<&str>, ws: Option<&str>, ts: u64| AnalyticsEvent {
                id: Id::new(id),
                event_name: name.to_string(),
                agent_id: agent.map(str::to_string),
                user_id: None,
                session_id: None,
                workspace_id: ws.map(str::to_string),
                repo_id: None,
                properties: serde_json::json!({}),
                timestamp: ts,
            };
        for i in 0..150u64 {
            AnalyticsRepository::record(
                &s,
                &mk(
                    &format!("m{i}"),
                    "mr.merged",
                    Some("a1"),
                    Some("ws-1"),
                    1000 + i,
                ),
            )
            .await
            .unwrap();
        }
        for (i, agent) in [(0u64, Some("a2")), (1, None)] {
            AnalyticsRepository::record(
                &s,
                &mk(&format!("c{i}"), "mr.closed", agent, Some("ws-2"), 2000 + i),
            )
            .await
            .unwrap();
        }

        let f = AnalyticsQueryFilter {
            limit: 100,
            ..AnalyticsQueryFilter::new()
        };

        // Grouped totals cover the FULL filtered set (150 > limit 100).
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![("mr.closed".to_string(), 2), ("mr.merged".to_string(), 150)],
            "grouped totals must not be truncated by the event-level limit"
        );

        // The totals agree with count() on the same question. (i64::MAX,
        // not u64::MAX: the SQL adapters bind `until` as i64.)
        let n = AnalyticsRepository::count(&s, "mr.merged", 0, i64::MAX as u64)
            .await
            .unwrap();
        assert_eq!(n, 150);

        // limit bounds the number of returned groups, not the totals.
        let f = AnalyticsQueryFilter {
            limit: 1,
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(out, vec![("mr.closed".to_string(), 2)]);

        // NULL agent_id groups under "(none)".
        let f = AnalyticsQueryFilter::new();
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::AgentId)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![
                ("(none)".to_string(), 1),
                ("a1".to_string(), 150),
                ("a2".to_string(), 1)
            ]
        );

        // The grouped path applies the same filters as query_filtered:
        // prefix event_name + workspace scope.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m*".into()),
            workspace_id: Some("ws-1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(out, vec![("mr.merged".to_string(), 150)]);

        // Exact event_name filter (the `*` wildcard must not degrade to a
        // prefix match of everything).
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.closed".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::WorkspaceId)
            .await
            .unwrap();
        assert_eq!(out, vec![("ws-2".to_string(), 2)]);
    }

    #[tokio::test]
    async fn analytics_count_grouped_by_day() {
        let (_tmp, s) = setup();
        let mk = |id: &str, ts: u64| AnalyticsEvent {
            id: Id::new(id),
            event_name: "ev".to_string(),
            agent_id: None,
            user_id: None,
            session_id: None,
            workspace_id: None,
            repo_id: None,
            properties: serde_json::json!({}),
            timestamp: ts,
        };
        // Jan 1 2024 and Jan 2 2024 UTC.
        let day1: u64 = 1_704_067_200;
        AnalyticsRepository::record(&s, &mk("d1", day1))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &mk("d2", day1 + 3600))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &mk("d3", day1 + 86_400))
            .await
            .unwrap();

        let f = AnalyticsQueryFilter::new();
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::Day)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![("2024-01-01".to_string(), 2), ("2024-01-02".to_string(), 1)]
        );
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

    #[tokio::test]
    async fn analytics_event_name_prefix_is_literal_case_sensitive() {
        // task-146 review R4 regression: the trailing-`*` prefix match is
        // LITERAL and case-sensitive on every adapter. SQLite LIKE is
        // ASCII-case-insensitive and honors `%`/`_` wildcards, so the old
        // `LIKE '{prefix}%'` filter let `MR.*` and `mr.m_rged*` match
        // `mr.merged` — diverging from the mem adapter's `starts_with`
        // and Postgres's case-sensitive LIKE. Both query paths must now
        // reject case and wildcard variants while still matching the
        // literal prefix.
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("e1", "mr.merged", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("e2", "mr.closed", 200))
            .await
            .unwrap();

        // Literal prefix still matches.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert_eq!(out.len(), 1, "literal prefix match");
        assert_eq!(out[0].id, Id::new("e1"));

        // Case-variant prefix must NOT match (SQLite LIKE would match).
        let f = AnalyticsQueryFilter {
            event_name: Some("MR.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert!(
            out.is_empty(),
            "case-variant prefix must not match: {out:?}"
        );

        // Wildcard characters in the prefix must NOT broaden the match
        // (SQLite LIKE would treat `_` as any-single-char).
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m_rged*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert!(
            out.is_empty(),
            "wildcard-bearing prefix must not match: {out:?}"
        );
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.%*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::query_filtered(&s, &f).await.unwrap();
        assert!(
            out.is_empty(),
            "% in prefix is literal, not a wildcard: {out:?}"
        );

        // Same literal semantics on the grouped path.
        let f = AnalyticsQueryFilter {
            event_name: Some("MR.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert!(
            out.is_empty(),
            "grouped: case-variant prefix must not match"
        );
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m_rged*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert!(
            out.is_empty(),
            "grouped: wildcard-bearing prefix must not match"
        );
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = AnalyticsRepository::count_grouped(&s, &f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![("mr.closed".to_string(), 1), ("mr.merged".to_string(), 1)]
        );
    }

    #[tokio::test]
    async fn analytics_delete_older_than_purges_old_keeps_new() {
        let (_tmp, s) = setup();
        AnalyticsRepository::record(&s, &make_event("old", "ev", 100))
            .await
            .unwrap();
        AnalyticsRepository::record(&s, &make_event("new", "ev", 200))
            .await
            .unwrap();

        let deleted = AnalyticsRepository::delete_older_than(&s, 150)
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        let results = AnalyticsRepository::query(&s, None, None, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("new"));

        assert_eq!(
            AnalyticsRepository::delete_older_than(&s, 150)
                .await
                .unwrap(),
            0
        );
    }
}
