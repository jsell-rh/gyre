use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus};
use gyre_ports::AgentRepository;

use super::{open_conn, SqliteStorage};

fn status_to_str(s: &AgentStatus) -> &'static str {
    match s {
        AgentStatus::Idle => "Idle",
        AgentStatus::Active => "Active",
        AgentStatus::Blocked => "Blocked",
        AgentStatus::Error => "Error",
        AgentStatus::Dead => "Dead",
    }
}

fn str_to_status(s: &str) -> Result<AgentStatus> {
    match s {
        "Idle" => Ok(AgentStatus::Idle),
        "Active" => Ok(AgentStatus::Active),
        "Blocked" => Ok(AgentStatus::Blocked),
        "Error" => Ok(AgentStatus::Error),
        "Dead" => Ok(AgentStatus::Dead),
        other => Err(anyhow!("unknown agent status: {}", other)),
    }
}

fn row_to_agent(row: &rusqlite::Row<'_>) -> Result<Agent> {
    let status_str: String = row.get(2)?;
    Ok(Agent {
        id: Id::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        status: str_to_status(&status_str)?,
        parent_id: row.get::<_, Option<String>>(3)?.map(Id::new),
        current_task_id: row.get::<_, Option<String>>(4)?.map(Id::new),
        lifetime_budget_secs: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        spawned_at: row.get::<_, i64>(6)? as u64,
        last_heartbeat: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
    })
}

#[async_trait]
impl AgentRepository for SqliteStorage {
    async fn create(&self, agent: &Agent) -> Result<()> {
        let path = self.db_path();
        let a = agent.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO agents (id, name, status, parent_id, current_task_id,
                                     lifetime_budget_secs, spawned_at, last_heartbeat)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    a.id.as_str(),
                    a.name,
                    status_to_str(&a.status),
                    a.parent_id.as_ref().map(|id| id.as_str()),
                    a.current_task_id.as_ref().map(|id| id.as_str()),
                    a.lifetime_budget_secs.map(|v| v as i64),
                    a.spawned_at as i64,
                    a.last_heartbeat.map(|v| v as i64),
                ],
            )
            .context("insert agent")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Agent>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, status, parent_id, current_task_id,
                        lifetime_budget_secs, spawned_at, last_heartbeat
                 FROM agents WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_agent(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>> {
        let path = self.db_path();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Agent>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, status, parent_id, current_task_id,
                        lifetime_budget_secs, spawned_at, last_heartbeat
                 FROM agents WHERE name = ?1",
            )?;
            let mut rows = stmt.query([&name])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_agent(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Agent>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<Agent>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, status, parent_id, current_task_id,
                        lifetime_budget_secs, spawned_at, last_heartbeat
                 FROM agents ORDER BY spawned_at",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_agent(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>> {
        let path = self.db_path();
        let status_str = status_to_str(status).to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Agent>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, status, parent_id, current_task_id,
                        lifetime_budget_secs, spawned_at, last_heartbeat
                 FROM agents WHERE status = ?1 ORDER BY spawned_at",
            )?;
            let rows = stmt.query_map([&status_str], |row| Ok(row_to_agent(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, agent: &Agent) -> Result<()> {
        let path = self.db_path();
        let a = agent.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE agents SET name=?1, status=?2, parent_id=?3, current_task_id=?4,
                          lifetime_budget_secs=?5, last_heartbeat=?6
                 WHERE id=?7",
                rusqlite::params![
                    a.name,
                    status_to_str(&a.status),
                    a.parent_id.as_ref().map(|id| id.as_str()),
                    a.current_task_id.as_ref().map(|id| id.as_str()),
                    a.lifetime_budget_secs.map(|v| v as i64),
                    a.last_heartbeat.map(|v| v as i64),
                    a.id.as_str(),
                ],
            )
            .context("update agent")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM agents WHERE id=?1", [id.as_str()])
                .context("delete agent")?;
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

    fn make_agent(id: &str, name: &str) -> Agent {
        Agent::new(Id::new(id), name, 1000)
    }

    #[tokio::test]
    async fn create_and_find_by_id() {
        let (_tmp, s) = setup();
        let a = make_agent("a1", "worker");
        s.create(&a).await.unwrap();
        let found = s.find_by_id(&a.id).await.unwrap().unwrap();
        assert_eq!(found.name, "worker");
        assert_eq!(found.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn find_by_name() {
        let (_tmp, s) = setup();
        let a = make_agent("a1", "scout");
        s.create(&a).await.unwrap();
        let found = s.find_by_name("scout").await.unwrap().unwrap();
        assert_eq!(found.id, a.id);
    }

    #[tokio::test]
    async fn find_by_name_missing() {
        let (_tmp, s) = setup();
        assert!(s.find_by_name("ghost").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_agents() {
        let (_tmp, s) = setup();
        s.create(&make_agent("a1", "alice")).await.unwrap();
        s.create(&make_agent("a2", "bob")).await.unwrap();
        let all = s.list().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_by_status() {
        let (_tmp, s) = setup();
        let mut a1 = make_agent("a1", "alice");
        let a2 = make_agent("a2", "bob");
        s.create(&a1).await.unwrap();
        s.create(&a2).await.unwrap();
        a1.status = AgentStatus::Active;
        s.update(&a1).await.unwrap();

        let active = s.list_by_status(&AgentStatus::Active).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "alice");

        let idle = s.list_by_status(&AgentStatus::Idle).await.unwrap();
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].name, "bob");
    }

    #[tokio::test]
    async fn update_agent() {
        let (_tmp, s) = setup();
        let mut a = make_agent("a1", "alpha");
        s.create(&a).await.unwrap();
        a.status = AgentStatus::Active;
        a.last_heartbeat = Some(9999);
        s.update(&a).await.unwrap();
        let found = s.find_by_id(&a.id).await.unwrap().unwrap();
        assert_eq!(found.status, AgentStatus::Active);
        assert_eq!(found.last_heartbeat, Some(9999));
    }

    #[tokio::test]
    async fn delete_agent() {
        let (_tmp, s) = setup();
        let a = make_agent("a1", "temp");
        s.create(&a).await.unwrap();
        s.delete(&a.id).await.unwrap();
        assert!(s.find_by_id(&a.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn agent_with_parent() {
        let (_tmp, s) = setup();
        let parent = make_agent("p1", "parent");
        let mut child = make_agent("c1", "child");
        child.parent_id = Some(Id::new("p1"));
        s.create(&parent).await.unwrap();
        s.create(&child).await.unwrap();
        let found = s.find_by_id(&child.id).await.unwrap().unwrap();
        assert_eq!(found.parent_id, Some(Id::new("p1")));
    }
}
