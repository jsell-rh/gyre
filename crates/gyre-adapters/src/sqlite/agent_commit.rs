use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::AgentCommit;
use gyre_ports::AgentCommitRepository;

use super::{open_conn, SqliteStorage};

fn row_to_agent_commit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentCommit> {
    Ok(AgentCommit {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        repository_id: Id::new(row.get::<_, String>(2)?),
        commit_sha: row.get(3)?,
        branch: row.get(4)?,
        timestamp: row.get::<_, i64>(5)? as u64,
    })
}

#[async_trait]
impl AgentCommitRepository for SqliteStorage {
    async fn record(&self, mapping: &AgentCommit) -> Result<()> {
        let path = self.db_path();
        let m = mapping.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO agent_commits (id, agent_id, repository_id, commit_sha, branch, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    m.id.as_str(),
                    m.agent_id.as_str(),
                    m.repository_id.as_str(),
                    m.commit_sha,
                    m.branch,
                    m.timestamp as i64,
                ],
            )
            .context("insert agent_commit")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentCommit>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp
                 FROM agent_commits WHERE agent_id = ?1 ORDER BY timestamp DESC",
            )?;
            let rows = stmt.query_map([agent_id.as_str()], row_to_agent_commit)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>> {
        let path = self.db_path();
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentCommit>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp
                 FROM agent_commits WHERE repository_id = ?1 ORDER BY timestamp DESC",
            )?;
            let rows = stmt.query_map([repo_id.as_str()], row_to_agent_commit)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>> {
        let path = self.db_path();
        let sha = sha.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<AgentCommit>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp
                 FROM agent_commits WHERE commit_sha = ?1",
            )?;
            let mut rows = stmt.query([sha.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_agent_commit(row)?))
            } else {
                Ok(None)
            }
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

    fn make_commit(id: &str, agent_id: &str, repo_id: &str, sha: &str) -> AgentCommit {
        AgentCommit::new(
            Id::new(id),
            Id::new(agent_id),
            Id::new(repo_id),
            sha,
            "main",
            1000,
        )
    }

    #[tokio::test]
    async fn record_and_find_by_agent() {
        let (_tmp, s) = setup();
        let ac = make_commit("c1", "agent1", "repo1", "deadbeef");
        AgentCommitRepository::record(&s, &ac).await.unwrap();

        let results = AgentCommitRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].commit_sha, "deadbeef");
    }

    #[tokio::test]
    async fn record_and_find_by_repo() {
        let (_tmp, s) = setup();
        let ac1 = make_commit("c1", "agent1", "repo1", "aaa");
        let ac2 = make_commit("c2", "agent2", "repo1", "bbb");
        let ac3 = make_commit("c3", "agent1", "repo2", "ccc");
        AgentCommitRepository::record(&s, &ac1).await.unwrap();
        AgentCommitRepository::record(&s, &ac2).await.unwrap();
        AgentCommitRepository::record(&s, &ac3).await.unwrap();

        let results = AgentCommitRepository::find_by_repo(&s, &Id::new("repo1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn find_by_commit_sha() {
        let (_tmp, s) = setup();
        let ac = make_commit("c1", "agent1", "repo1", "sha123");
        AgentCommitRepository::record(&s, &ac).await.unwrap();

        let found = AgentCommitRepository::find_by_commit(&s, "sha123")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().agent_id, Id::new("agent1"));
    }

    #[tokio::test]
    async fn find_by_commit_missing() {
        let (_tmp, s) = setup();
        let found = AgentCommitRepository::find_by_commit(&s, "nope")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn find_by_agent_empty() {
        let (_tmp, s) = setup();
        let results = AgentCommitRepository::find_by_agent(&s, &Id::new("nobody"))
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn multiple_commits_same_agent() {
        let (_tmp, s) = setup();
        for i in 0..3 {
            let ac = make_commit(&format!("c{i}"), "agent1", "repo1", &format!("sha{i}"));
            AgentCommitRepository::record(&s, &ac).await.unwrap();
        }
        let results = AgentCommitRepository::find_by_agent(&s, &Id::new("agent1"))
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
    }
}
