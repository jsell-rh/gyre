use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::ActivityEvent;
use gyre_ports::activity::{ActivityQuery, ActivityRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::activity_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = activity_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl ActivityRepository for PgStorage {
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
