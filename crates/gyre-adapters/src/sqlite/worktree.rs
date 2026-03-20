use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::AgentWorktree;
use gyre_ports::WorktreeRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::agent_worktrees;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_worktrees)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AgentWorktreeRow {
    id: String,
    agent_id: String,
    repository_id: String,
    task_id: Option<String>,
    branch: String,
    path: String,
    created_at: i64,
}

impl From<AgentWorktreeRow> for AgentWorktree {
    fn from(r: AgentWorktreeRow) -> Self {
        AgentWorktree {
            id: Id::new(r.id),
            agent_id: Id::new(r.agent_id),
            repository_id: Id::new(r.repository_id),
            task_id: r.task_id.map(Id::new),
            branch: r.branch,
            path: r.path,
            created_at: r.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = agent_worktrees)]
struct AgentWorktreeRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    repository_id: &'a str,
    task_id: Option<&'a str>,
    branch: &'a str,
    path: &'a str,
    created_at: i64,
}

impl<'a> From<&'a AgentWorktree> for AgentWorktreeRecord<'a> {
    fn from(w: &'a AgentWorktree) -> Self {
        AgentWorktreeRecord {
            id: w.id.as_str(),
            agent_id: w.agent_id.as_str(),
            repository_id: w.repository_id.as_str(),
            task_id: w.task_id.as_ref().map(|id| id.as_str()),
            branch: &w.branch,
            path: &w.path,
            created_at: w.created_at as i64,
        }
    }
}

#[async_trait]
impl WorktreeRepository for SqliteStorage {
    async fn create(&self, worktree: &AgentWorktree) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let w = worktree.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = AgentWorktreeRecord::from(&w);
            diesel::insert_into(agent_worktrees::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert agent_worktree")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentWorktree>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentWorktree>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_worktrees::table
                .filter(agent_worktrees::agent_id.eq(agent_id.as_str()))
                .order(agent_worktrees::created_at.asc())
                .load::<AgentWorktreeRow>(&mut *conn)
                .context("find worktrees by agent")?;
            Ok(rows.into_iter().map(AgentWorktree::from).collect())
        })
        .await?
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentWorktree>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentWorktree>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_worktrees::table
                .filter(agent_worktrees::repository_id.eq(repo_id.as_str()))
                .order(agent_worktrees::created_at.asc())
                .load::<AgentWorktreeRow>(&mut *conn)
                .context("find worktrees by repo")?;
            Ok(rows.into_iter().map(AgentWorktree::from).collect())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(agent_worktrees::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete agent_worktree")?;
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

    fn make_worktree(id: &str, agent_id: &str, repo_id: &str, branch: &str) -> AgentWorktree {
        AgentWorktree::new(
            Id::new(id),
            Id::new(agent_id),
            Id::new(repo_id),
            None,
            branch,
            format!("/tmp/wt/{id}"),
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find_by_agent() {
        let (_tmp, s) = setup();
        let wt = make_worktree("wt1", "agent1", "repo1", "feat/x");
        WorktreeRepository::create(&s, &wt).await.unwrap();

        let results = WorktreeRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].branch, "feat/x");
        assert_eq!(results[0].path, "/tmp/wt/wt1");
    }

    #[tokio::test]
    async fn create_and_find_by_repo() {
        let (_tmp, s) = setup();
        let wt1 = make_worktree("wt1", "agent1", "repo1", "main");
        let wt2 = make_worktree("wt2", "agent2", "repo1", "feat/y");
        let wt3 = make_worktree("wt3", "agent1", "repo2", "main");
        WorktreeRepository::create(&s, &wt1).await.unwrap();
        WorktreeRepository::create(&s, &wt2).await.unwrap();
        WorktreeRepository::create(&s, &wt3).await.unwrap();

        let results = WorktreeRepository::find_by_repo(&s, &Id::new("repo1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn create_and_delete() {
        let (_tmp, s) = setup();
        let wt = make_worktree("wt1", "agent1", "repo1", "main");
        WorktreeRepository::create(&s, &wt).await.unwrap();
        WorktreeRepository::delete(&s, &Id::new("wt1"))
            .await
            .unwrap();

        let results = WorktreeRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn worktree_with_task_id() {
        let (_tmp, s) = setup();
        let wt = AgentWorktree::new(
            Id::new("wt1"),
            Id::new("agent1"),
            Id::new("repo1"),
            Some(Id::new("task42")),
            "feat/task42",
            "/tmp/wt/task42",
            5000,
        );
        WorktreeRepository::create(&s, &wt).await.unwrap();

        let results = WorktreeRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, Some(Id::new("task42")));
    }

    #[tokio::test]
    async fn find_by_agent_empty() {
        let (_tmp, s) = setup();
        let results = WorktreeRepository::find_by_agent(&s, &Id::new("nobody"))
            .await
            .unwrap();
        assert!(results.is_empty());
    }
}
