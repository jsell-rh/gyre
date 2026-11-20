use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Session;
use gyre_ports::AgentSessionRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::agent_sessions;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SessionRow {
    id: String,
    agent_id: String,
    repository_id: String,
    started_at: i64,
    ended_at: Option<i64>,
    initial_branch: String,
    final_branch: Option<String>,
    dirty: Option<bool>,
    uncommitted_paths: String,
    dirty_detail: Option<String>,
    cost: Option<f64>,
    tokens_used: Option<i64>,
    messages_read: String,
}

impl From<SessionRow> for Session {
    fn from(r: SessionRow) -> Self {
        Session {
            id: Id::new(r.id),
            agent_id: Id::new(r.agent_id),
            repository_id: Id::new(r.repository_id),
            started_at: r.started_at as u64,
            ended_at: r.ended_at.map(|t| t as u64),
            initial_branch: r.initial_branch,
            final_branch: r.final_branch,
            dirty: r.dirty,
            uncommitted_paths: serde_json::from_str(&r.uncommitted_paths).unwrap_or_default(),
            dirty_detail: r.dirty_detail,
            cost: r.cost,
            tokens_used: r.tokens_used.map(|t| t as u64),
            messages_read: serde_json::from_str(&r.messages_read).unwrap_or_default(),
        }
    }
}

#[derive(Insertable, AsChangeset)]
#[diesel(treat_none_as_null = true)]
#[diesel(table_name = agent_sessions)]
struct SessionRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    repository_id: &'a str,
    started_at: i64,
    ended_at: Option<i64>,
    initial_branch: &'a str,
    final_branch: Option<&'a str>,
    dirty: Option<bool>,
    uncommitted_paths: String,
    dirty_detail: Option<&'a str>,
    cost: Option<f64>,
    tokens_used: Option<i64>,
    messages_read: String,
}

impl<'a> From<&'a Session> for SessionRecord<'a> {
    fn from(s: &'a Session) -> Self {
        SessionRecord {
            id: s.id.as_str(),
            agent_id: s.agent_id.as_str(),
            repository_id: s.repository_id.as_str(),
            started_at: s.started_at as i64,
            ended_at: s.ended_at.map(|t| t as i64),
            initial_branch: &s.initial_branch,
            final_branch: s.final_branch.as_deref(),
            dirty: s.dirty,
            uncommitted_paths: serde_json::to_string(&s.uncommitted_paths)
                .unwrap_or_else(|_| "[]".to_string()),
            dirty_detail: s.dirty_detail.as_deref(),
            cost: s.cost,
            tokens_used: s.tokens_used.map(|t| t as i64),
            messages_read: serde_json::to_string(&s.messages_read)
                .unwrap_or_else(|_| "[]".to_string()),
        }
    }
}

#[async_trait]
impl AgentSessionRepository for PgStorage {
    async fn create(&self, session: &Session) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let s = session.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fails if a session with the same id already
            // exists (same guard as the SQLite and mem adapters).
            let exists = agent_sessions::table
                .filter(agent_sessions::id.eq(s.id.as_str()))
                .select(agent_sessions::id)
                .first::<String>(&mut *conn)
                .optional()
                .context("check agent_session existence")?;
            if exists.is_some() {
                anyhow::bail!("session {} already exists", s.id);
            }
            let record = SessionRecord::from(&s);
            diesel::insert_into(agent_sessions::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert agent_session")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<Session>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Session>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_sessions::table
                .filter(agent_sessions::agent_id.eq(agent_id.as_str()))
                // Chronological (oldest first). Same-second ties break by
                // session window: an ended session predates a still-open
                // session started in the same second (one agent's sessions
                // are sequential context windows) — ended_at ASC NULLS LAST
                // via is_null (false sorts first on both SQLite and
                // PostgreSQL), then ended_at, then the id as the final
                // deterministic fallback. Sorting on the random-UUID id
                // alone orders same-second sessions arbitrarily.
                .order(agent_sessions::started_at.asc())
                .then_order_by(agent_sessions::ended_at.is_null().asc())
                .then_order_by(agent_sessions::ended_at.asc())
                .then_order_by(agent_sessions::id.asc())
                .load::<SessionRow>(&mut *conn)
                .context("find agent_sessions by agent")?;
            Ok(rows.into_iter().map(Session::from).collect())
        })
        .await?
    }

    async fn find_by_repo(&self, repository_id: &Id) -> Result<Vec<Session>> {
        let pool = Arc::clone(&self.pool);
        let repository_id = repository_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Session>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_sessions::table
                .filter(agent_sessions::repository_id.eq(repository_id.as_str()))
                // Same tie-break as find_by_agent (see there).
                .order(agent_sessions::started_at.asc())
                .then_order_by(agent_sessions::ended_at.is_null().asc())
                .then_order_by(agent_sessions::ended_at.asc())
                .then_order_by(agent_sessions::id.asc())
                .load::<SessionRow>(&mut *conn)
                .context("find agent_sessions by repo")?;
            Ok(rows.into_iter().map(Session::from).collect())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Session>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Session>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = agent_sessions::table
                .filter(agent_sessions::id.eq(id.as_str()))
                .first::<SessionRow>(&mut *conn)
                .optional()
                .context("find agent_session by id")?;
            Ok(result.map(Session::from))
        })
        .await?
    }

    async fn find_open_by_agent(&self, agent_id: &Id) -> Result<Option<Session>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Session>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = agent_sessions::table
                .filter(agent_sessions::agent_id.eq(agent_id.as_str()))
                .filter(agent_sessions::ended_at.is_null())
                .order(agent_sessions::started_at.desc())
                .first::<SessionRow>(&mut *conn)
                .optional()
                .context("find open agent_session by agent")?;
            Ok(result.map(Session::from))
        })
        .await?
    }

    async fn update(&self, session: &Session) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let s = session.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = SessionRecord::from(&s);
            let rows = diesel::update(agent_sessions::table)
                .filter(agent_sessions::id.eq(s.id.as_str()))
                .set(&record)
                .execute(&mut *conn)
                .context("update agent_session")?;
            // Port contract: fails if the session does not exist.
            if rows == 0 {
                anyhow::bail!("session {} does not exist", s.id);
            }
            Ok(())
        })
        .await?
    }
}
