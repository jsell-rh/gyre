use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Session;
use gyre_ports::AgentSessionRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::agent_sessions;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_sessions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl AgentSessionRepository for SqliteStorage {
    async fn create(&self, session: &Session) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let s = session.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fails if a session with the same id already
            // exists. The primary key rejects the duplicate insert; the
            // existence check turns it into the contract's error.
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
                // The latest open session (there is at most one per agent,
                // but the query must return the newest if several exist).
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

    fn make_session(id: &str, agent_id: &str, repo_id: &str, started_at: u64) -> Session {
        Session::new(
            Id::new(id),
            Id::new(agent_id),
            Id::new(repo_id),
            started_at,
            "feat/x",
        )
    }

    #[tokio::test]
    async fn create_and_find_by_agent_chronological() {
        let (_tmp, s) = setup();
        let s1 = make_session("s1", "agent1", "repo1", 1000);
        let s2 = make_session("s2", "agent1", "repo1", 2000);
        let s3 = make_session("s3", "agent2", "repo1", 3000);
        AgentSessionRepository::create(&s, &s2).await.unwrap();
        AgentSessionRepository::create(&s, &s1).await.unwrap();
        AgentSessionRepository::create(&s, &s3).await.unwrap();

        let results = AgentSessionRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, Id::new("s1"));
        assert_eq!(results[1].id, Id::new("s2"));
    }

    /// Same-second ordering: started_at has second granularity, so a
    /// terminated session and its respawn successor can share a
    /// started_at. The tie must break by session window — the ended
    /// session predates the still-open one — never by the random-UUID row
    /// id, which orders the tie arbitrarily (CI run 44611030853,
    /// respawn_opens_a_new_session: "session 1 stays terminated"). The
    /// ids here contradict the window order ("aa-…" is the OPEN session),
    /// so a bare id tiebreak fails this test deterministically.
    #[tokio::test]
    async fn same_second_tie_orders_ended_session_before_open() {
        let (_tmp, s) = setup();
        // Both sessions started in the same wall-clock second.
        let same_second = 1_800_000_000u64;
        let mut ended = make_session("zz-ended", "agent1", "repo1", same_second);
        ended.ended_at = Some(same_second + 1);
        let open = make_session("aa-open", "agent1", "repo1", same_second);
        // Insertion order opposite to the expected result.
        AgentSessionRepository::create(&s, &open).await.unwrap();
        AgentSessionRepository::create(&s, &ended).await.unwrap();

        let results = AgentSessionRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(
            results.iter().map(|x| x.id.to_string()).collect::<Vec<_>>(),
            vec!["zz-ended".to_string(), "aa-open".to_string()],
            "same-second sessions order by session window, not random id"
        );
        assert_eq!(results[0].ended_at, Some(same_second + 1));
        assert_eq!(results[1].ended_at, None);

        // Same tie-break on the by-repo query.
        let repo_sessions = AgentSessionRepository::find_by_repo(&s, &Id::new("repo1"))
            .await
            .unwrap();
        assert_eq!(
            repo_sessions
                .iter()
                .map(|x| x.id.to_string())
                .collect::<Vec<_>>(),
            vec!["zz-ended".to_string(), "aa-open".to_string()]
        );
    }

    /// Same-second tie between two ENDED sessions: the one that ended
    /// earlier is the earlier context window.
    #[tokio::test]
    async fn same_second_tie_between_ended_sessions_orders_by_ended_at() {
        let (_tmp, s) = setup();
        let same_second = 1_800_000_000u64;
        let mut early = make_session("zz-early", "agent1", "repo1", same_second);
        early.ended_at = Some(same_second);
        let mut late = make_session("aa-late", "agent1", "repo1", same_second);
        late.ended_at = Some(same_second + 1);
        AgentSessionRepository::create(&s, &late).await.unwrap();
        AgentSessionRepository::create(&s, &early).await.unwrap();

        let results = AgentSessionRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(
            results.iter().map(|x| x.id.to_string()).collect::<Vec<_>>(),
            vec!["zz-early".to_string(), "aa-late".to_string()],
            "same-second ended sessions order by ended_at, not random id"
        );
    }

    #[tokio::test]
    async fn create_duplicate_id_fails() {
        let (_tmp, s) = setup();
        let s1 = make_session("s1", "agent1", "repo1", 1000);
        AgentSessionRepository::create(&s, &s1).await.unwrap();
        let dup = make_session("s1", "agent1", "repo1", 2000);
        let err = AgentSessionRepository::create(&s, &dup).await;
        assert!(err.is_err(), "duplicate session id must fail");
    }

    #[tokio::test]
    async fn find_by_id_and_repo() {
        let (_tmp, s) = setup();
        let s1 = make_session("s1", "agent1", "repo1", 1000);
        let s2 = make_session("s2", "agent2", "repo2", 1500);
        AgentSessionRepository::create(&s, &s1).await.unwrap();
        AgentSessionRepository::create(&s, &s2).await.unwrap();

        let found = AgentSessionRepository::find_by_id(&s, &Id::new("s1"))
            .await
            .unwrap();
        assert!(found.is_some());

        let repo_sessions = AgentSessionRepository::find_by_repo(&s, &Id::new("repo1"))
            .await
            .unwrap();
        assert_eq!(repo_sessions.len(), 1);
        assert_eq!(repo_sessions[0].id, Id::new("s1"));
    }

    #[tokio::test]
    async fn find_open_by_agent() {
        let (_tmp, s) = setup();
        let mut s1 = make_session("s1", "agent1", "repo1", 1000);
        s1.ended_at = Some(1500);
        let s2 = make_session("s2", "agent1", "repo1", 2000);
        AgentSessionRepository::create(&s, &s1).await.unwrap();
        AgentSessionRepository::create(&s, &s2).await.unwrap();

        // Only the session without ended_at is open.
        let open = AgentSessionRepository::find_open_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap()
            .expect("open session must resolve");
        assert_eq!(open.id, Id::new("s2"));

        // Ending it leaves no open session.
        let mut ended = open;
        ended.ended_at = Some(2500);
        AgentSessionRepository::update(&s, &ended).await.unwrap();
        let open = AgentSessionRepository::find_open_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert!(open.is_none());
    }

    #[tokio::test]
    async fn update_termination_state_round_trip() {
        let (_tmp, s) = setup();
        let s1 = make_session("s1", "agent1", "repo1", 1000);
        AgentSessionRepository::create(&s, &s1).await.unwrap();

        let mut ended = AgentSessionRepository::find_by_id(&s, &Id::new("s1"))
            .await
            .unwrap()
            .unwrap();
        ended.ended_at = Some(1800);
        ended.final_branch = Some("feat/moved".to_string());
        ended.dirty = Some(true);
        ended.uncommitted_paths = vec!["src/lib.rs".to_string()];
        ended.dirty_detail = None;
        AgentSessionRepository::update(&s, &ended).await.unwrap();

        let reloaded = AgentSessionRepository::find_by_id(&s, &Id::new("s1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.ended_at, Some(1800));
        assert_eq!(reloaded.final_branch.as_deref(), Some("feat/moved"));
        assert_eq!(reloaded.dirty, Some(true));
        assert_eq!(reloaded.uncommitted_paths, vec!["src/lib.rs".to_string()]);

        // Unknown dirty state (missing worktree) round-trips as NULL +
        // detail — never a silent clean.
        let mut unknown = reloaded;
        unknown.dirty = None;
        unknown.uncommitted_paths = Vec::new();
        unknown.dirty_detail = Some("worktree missing".to_string());
        AgentSessionRepository::update(&s, &unknown).await.unwrap();
        let reloaded = AgentSessionRepository::find_by_id(&s, &Id::new("s1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.dirty, None);
        assert_eq!(reloaded.dirty_detail.as_deref(), Some("worktree missing"));
    }

    #[tokio::test]
    async fn update_missing_session_fails() {
        let (_tmp, s) = setup();
        let ghost = make_session("ghost", "agent1", "repo1", 1000);
        let err = AgentSessionRepository::update(&s, &ghost).await;
        assert!(err.is_err(), "updating a nonexistent session must fail");
    }
}
