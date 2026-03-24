use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::AgentCommit;
use gyre_ports::AgentCommitRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::agent_commits;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_commits)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AgentCommitRow {
    id: String,
    agent_id: String,
    repository_id: String,
    commit_sha: String,
    branch: String,
    timestamp: i64,
    task_id: Option<String>,
    spawned_by_user_id: Option<String>,
    parent_agent_id: Option<String>,
    model_context: Option<String>,
    attestation_level: Option<String>,
}

impl From<AgentCommitRow> for AgentCommit {
    fn from(r: AgentCommitRow) -> Self {
        AgentCommit {
            id: Id::new(r.id),
            agent_id: Id::new(r.agent_id),
            repository_id: Id::new(r.repository_id),
            commit_sha: r.commit_sha,
            branch: r.branch,
            timestamp: r.timestamp as u64,
            task_id: r.task_id,
            spawned_by_user_id: r.spawned_by_user_id,
            parent_agent_id: r.parent_agent_id,
            model_context: r.model_context,
            attestation_level: r.attestation_level,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = agent_commits)]
struct AgentCommitRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    repository_id: &'a str,
    commit_sha: &'a str,
    branch: &'a str,
    timestamp: i64,
    task_id: Option<&'a str>,
    spawned_by_user_id: Option<&'a str>,
    parent_agent_id: Option<&'a str>,
    model_context: Option<&'a str>,
    attestation_level: Option<&'a str>,
}

impl<'a> From<&'a AgentCommit> for AgentCommitRecord<'a> {
    fn from(m: &'a AgentCommit) -> Self {
        AgentCommitRecord {
            id: m.id.as_str(),
            agent_id: m.agent_id.as_str(),
            repository_id: m.repository_id.as_str(),
            commit_sha: &m.commit_sha,
            branch: &m.branch,
            timestamp: m.timestamp as i64,
            task_id: m.task_id.as_deref(),
            spawned_by_user_id: m.spawned_by_user_id.as_deref(),
            parent_agent_id: m.parent_agent_id.as_deref(),
            model_context: m.model_context.as_deref(),
            attestation_level: m.attestation_level.as_deref(),
        }
    }
}

#[async_trait]
impl AgentCommitRepository for SqliteStorage {
    async fn record(&self, mapping: &AgentCommit) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = mapping.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = AgentCommitRecord::from(&m);
            diesel::insert_into(agent_commits::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert agent_commit")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentCommit>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_commits::table
                .filter(agent_commits::agent_id.eq(agent_id.as_str()))
                .order(agent_commits::timestamp.desc())
                .load::<AgentCommitRow>(&mut *conn)
                .context("find agent_commits by agent")?;
            Ok(rows.into_iter().map(AgentCommit::from).collect())
        })
        .await?
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentCommit>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_commits::table
                .filter(agent_commits::repository_id.eq(repo_id.as_str()))
                .order(agent_commits::timestamp.desc())
                .load::<AgentCommitRow>(&mut *conn)
                .context("find agent_commits by repo")?;
            Ok(rows.into_iter().map(AgentCommit::from).collect())
        })
        .await?
    }

    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>> {
        let pool = Arc::clone(&self.pool);
        let sha = sha.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<AgentCommit>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = agent_commits::table
                .filter(agent_commits::commit_sha.eq(sha.as_str()))
                .first::<AgentCommitRow>(&mut *conn)
                .optional()
                .context("find agent_commit by sha")?;
            Ok(result.map(AgentCommit::from))
        })
        .await?
    }

    async fn find_by_task(&self, task_id: &str) -> Result<Vec<AgentCommit>> {
        let pool = Arc::clone(&self.pool);
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentCommit>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_commits::table
                .filter(agent_commits::task_id.eq(task_id.as_str()))
                .order(agent_commits::timestamp.desc())
                .load::<AgentCommitRow>(&mut *conn)
                .context("find agent_commits by task")?;
            Ok(rows.into_iter().map(AgentCommit::from).collect())
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

    #[tokio::test]
    async fn provenance_fields_round_trip() {
        let (_tmp, s) = setup();
        let ac = make_commit("c1", "agent1", "repo1", "abc").with_provenance(
            Some("task-42".to_string()),
            Some("user-7".to_string()),
            Some("parent-agent".to_string()),
            Some(r#"{"model":"claude-sonnet"}"#.to_string()),
        );
        AgentCommitRepository::record(&s, &ac).await.unwrap();

        let found = AgentCommitRepository::find_by_commit(&s, "abc")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.task_id.as_deref(), Some("task-42"));
        assert_eq!(found.spawned_by_user_id.as_deref(), Some("user-7"));
        assert_eq!(found.parent_agent_id.as_deref(), Some("parent-agent"));
        assert!(found.model_context.is_some());
    }

    #[tokio::test]
    async fn find_by_task_id() {
        let (_tmp, s) = setup();
        let ac1 = make_commit("c1", "agent1", "repo1", "sha1")
            .with_provenance(Some("task-A".to_string()), None, None, None);
        let ac2 = make_commit("c2", "agent2", "repo1", "sha2")
            .with_provenance(Some("task-A".to_string()), None, None, None);
        let ac3 = make_commit("c3", "agent1", "repo1", "sha3")
            .with_provenance(Some("task-B".to_string()), None, None, None);
        AgentCommitRepository::record(&s, &ac1).await.unwrap();
        AgentCommitRepository::record(&s, &ac2).await.unwrap();
        AgentCommitRepository::record(&s, &ac3).await.unwrap();

        let results = AgentCommitRepository::find_by_task(&s, "task-A")
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }
}
