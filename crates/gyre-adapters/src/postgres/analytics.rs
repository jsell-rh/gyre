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

use super::PgStorage;
use crate::schema::{analytics_events, cost_entries};

/// Escape SQL LIKE wildcards (`%`, `_`) and the escape character itself
/// in a user-supplied event-name prefix so it is matched literally
/// (Postgres LIKE's default escape character is the backslash).
/// Task-146 review R4: without escaping, a caller-supplied `mr.m_rged*`
/// filter would match `mr.merged` (`_` = any single char) — wildcard
/// semantics the mem and SQLite adapters do not implement.
fn escape_like_wildcards(prefix: &str) -> String {
    let mut escaped = String::with_capacity(prefix.len());
    for c in prefix.chars() {
        if c == '%' || c == '_' || c == '\\' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = analytics_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl AnalyticsRepository for PgStorage {
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
                user_id: e.user_id.as_deref(),
                session_id: e.session_id.as_deref(),
                workspace_id: e.workspace_id.as_deref(),
                repo_id: e.repo_id.as_deref(),
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
            if let Some(name) = &event_name {
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
            if let Some(name) = &filter.event_name {
                // Exact or trailing-`*` prefix match (analytics.md §Query
                // Parameters). Postgres LIKE is case-sensitive, matching
                // the mem adapter's `starts_with` and SQLite's substr
                // equality; the prefix's LIKE wildcards (`%`, `_`, `\\`)
                // are escaped (task-146 review R4) so a caller-supplied
                // wildcard cannot broaden the match — literal prefix
                // semantics on every adapter.
                if let Some(prefix) = name.strip_suffix('*') {
                    query = query.filter(
                        analytics_events::event_name
                            .like(format!("{}%", escape_like_wildcards(prefix))),
                    );
                } else {
                    query = query.filter(analytics_events::event_name.eq(name.as_str()));
                }
            }
            if let Some(id) = &filter.agent_id {
                query = query.filter(analytics_events::agent_id.eq(id.as_str()));
            }
            if let Some(id) = &filter.user_id {
                query = query.filter(analytics_events::user_id.eq(id.as_str()));
            }
            if let Some(id) = &filter.workspace_id {
                query = query.filter(analytics_events::workspace_id.eq(id.as_str()));
            }
            if let Some(id) = &filter.repo_id {
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
                AnalyticsGroupBy::Day => {
                    // UTC calendar day (port contract, same key as the
                    // SQLite `date(ts,'unixepoch')` and mem epoch-day
                    // adapters): `AT TIME ZONE 'UTC'` pins the rendering
                    // to UTC instead of the server session TimeZone
                    // (task-146 review R4 — without it a 23:30 UTC event
                    // lands on the next day on a non-UTC deployment).
                    "to_char(to_timestamp(timestamp) AT TIME ZONE 'UTC', 'YYYY-MM-DD')".to_string()
                }
            };
            let mut ph = 0usize;
            let mut placeholder = || {
                ph += 1;
                format!("${ph}")
            };
            let mut sql = format!(
                "SELECT {key_expr} AS k, COUNT(*) AS cnt \
                 FROM analytics_events WHERE 1=1"
            );
            if filter.event_name.is_some() {
                // Exact or trailing-`*` prefix match (analytics.md
                // §Query Parameters), same operator choice as
                // query_filtered: `=` for exact, case-sensitive LIKE with
                // escaped wildcards for prefix (task-146 review R4).
                let is_prefix = filter
                    .event_name
                    .as_deref()
                    .is_some_and(|n| n.ends_with('*'));
                sql.push_str(if is_prefix {
                    " AND event_name LIKE "
                } else {
                    " AND event_name = "
                });
                sql.push_str(&placeholder());
            }
            if filter.agent_id.is_some() {
                sql.push_str(&format!(" AND agent_id = {}", placeholder()));
            }
            if filter.user_id.is_some() {
                sql.push_str(&format!(" AND user_id = {}", placeholder()));
            }
            if filter.workspace_id.is_some() {
                sql.push_str(&format!(" AND workspace_id = {}", placeholder()));
            }
            if filter.repo_id.is_some() {
                sql.push_str(&format!(" AND repo_id = {}", placeholder()));
            }
            if filter.since.is_some() {
                sql.push_str(&format!(" AND timestamp >= {}", placeholder()));
            }
            if filter.until.is_some() {
                sql.push_str(&format!(" AND timestamp <= {}", placeholder()));
            }
            sql.push_str(&format!(" GROUP BY k ORDER BY k LIMIT {}", placeholder()));
            // into_boxed(): bind() returns Self, so binds can be applied
            // conditionally (diesel's documented loop-binding pattern).
            let mut query = diesel::sql_query(&sql).into_boxed();
            if let Some(name) = &filter.event_name {
                let pattern = match name.strip_suffix('*') {
                    Some(prefix) => format!("{}%", escape_like_wildcards(prefix)),
                    None => name.clone(),
                };
                query = query.bind::<Text, _>(pattern);
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
        event_name: Option<&str>,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>> {
        let pool = Arc::clone(&self.pool);
        let event_name = event_name.map(str::to_string);
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>> {
            let mut conn = pool.get().context("get db connection")?;
            // `None` event_name aggregates across ALL event names — the
            // total daily event volume form (analytics.md §Analytics
            // Dashboard).
            let rows = match event_name {
                Some(name) => diesel::sql_query(
                    // UTC calendar day (task-146 review R4): pin the rendering
                    // to UTC, not the server session TimeZone — same key as
                    // the SQLite/mem adapters and the port contract doc.
                    "SELECT to_char(to_timestamp(timestamp) AT TIME ZONE 'UTC', 'YYYY-MM-DD') as day, COUNT(*) as cnt \
                     FROM analytics_events \
                     WHERE event_name = $1 AND timestamp >= $2 AND timestamp <= $3 \
                     GROUP BY day ORDER BY day",
                )
                .bind::<Text, _>(name)
                .bind::<BigInt, _>(since as i64)
                .bind::<BigInt, _>(until as i64)
                .load::<DayCount>(&mut *conn)
                .context("aggregate analytics_events by day")?,
                None => diesel::sql_query(
                    "SELECT to_char(to_timestamp(timestamp) AT TIME ZONE 'UTC', 'YYYY-MM-DD') as day, COUNT(*) as cnt \
                     FROM analytics_events \
                     WHERE timestamp >= $1 AND timestamp <= $2 \
                     GROUP BY day ORDER BY day",
                )
                .bind::<BigInt, _>(since as i64)
                .bind::<BigInt, _>(until as i64)
                .load::<DayCount>(&mut *conn)
                .context("aggregate analytics_events by day")?,
            };
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
impl CostRepository for PgStorage {
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
