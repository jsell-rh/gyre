use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::AgentWorktree;
use gyre_ports::WorktreeRepository;

use super::{open_conn, SqliteStorage};

fn row_to_worktree(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentWorktree> {
    let task_id: Option<String> = row.get(3)?;
    Ok(AgentWorktree {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        repository_id: Id::new(row.get::<_, String>(2)?),
        task_id: task_id.map(Id::new),
        branch: row.get(4)?,
        path: row.get(5)?,
        created_at: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl WorktreeRepository for SqliteStorage {
    async fn create(&self, worktree: &AgentWorktree) -> Result<()> {
        let path = self.db_path();
        let w = worktree.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO agent_worktrees (id, agent_id, repository_id, task_id, branch, path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    w.id.as_str(),
                    w.agent_id.as_str(),
                    w.repository_id.as_str(),
                    w.task_id.as_ref().map(|id| id.as_str()),
                    w.branch,
                    w.path,
                    w.created_at as i64,
                ],
            )
            .context("insert agent_worktree")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentWorktree>> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentWorktree>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, repository_id, task_id, branch, path, created_at
                 FROM agent_worktrees WHERE agent_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([agent_id.as_str()], row_to_worktree)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentWorktree>> {
        let path = self.db_path();
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentWorktree>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, repository_id, task_id, branch, path, created_at
                 FROM agent_worktrees WHERE repository_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([repo_id.as_str()], row_to_worktree)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM agent_worktrees WHERE id = ?1", [id.as_str()])
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
