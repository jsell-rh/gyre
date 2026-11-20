//! SQLite adapter for the `AgentInstanceRepository` port (TASK-273,
//! admin-panel.md: durable spawned-instance handles for orphan sweeps).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{AgentInstanceRecord, AgentInstanceRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::agent_instances;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_instances)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AgentInstanceRow {
    agent_id: String,
    backend: String,
    target_type: String,
    handle_id: String,
    pid: Option<i32>,
    spawned_at: i64,
    backend_config: Option<String>,
    terminated_at: Option<i64>,
    terminated_reason: Option<String>,
}

impl AgentInstanceRow {
    fn into_record(self) -> AgentInstanceRecord {
        AgentInstanceRecord {
            agent_id: self.agent_id,
            backend: self.backend,
            target_type: self.target_type,
            handle_id: self.handle_id,
            pid: self.pid.map(|p| p as u32),
            spawned_at: self.spawned_at as u64,
            backend_config: self.backend_config,
            terminated_at: self.terminated_at.map(|t| t as u64),
            terminated_reason: self.terminated_reason,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = agent_instances)]
struct NewAgentInstanceRow<'a> {
    agent_id: &'a str,
    backend: &'a str,
    target_type: &'a str,
    handle_id: &'a str,
    pid: Option<i32>,
    spawned_at: i64,
    backend_config: Option<&'a str>,
    terminated_at: Option<i64>,
    terminated_reason: Option<&'a str>,
}

#[async_trait]
impl AgentInstanceRepository for SqliteStorage {
    async fn record_spawned(&self, record: &AgentInstanceRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let record = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewAgentInstanceRow {
                agent_id: &record.agent_id,
                backend: &record.backend,
                target_type: &record.target_type,
                handle_id: &record.handle_id,
                pid: record.pid.map(|p| p as i32),
                spawned_at: record.spawned_at as i64,
                backend_config: record.backend_config.as_deref(),
                terminated_at: record.terminated_at.map(|t| t as i64),
                terminated_reason: record.terminated_reason.as_deref(),
            };
            // Replace on respawn: one live instance per agent.
            diesel::replace_into(agent_instances::table)
                .values(&row)
                .execute(&mut conn)
                .context("insert agent_instance")?;
            Ok(())
        })
        .await?
    }

    async fn mark_terminated(&self, agent_id: &str, reason: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let n = diesel::update(
                agent_instances::table.filter(agent_instances::agent_id.eq(agent_id.clone())),
            )
            .set((
                agent_instances::terminated_at.eq(now),
                agent_instances::terminated_reason.eq(reason),
            ))
            .execute(&mut conn)
            .context("mark agent_instance terminated")?;
            anyhow::ensure!(n > 0, "agent instance for '{agent_id}' not found");
            Ok(())
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &str) -> Result<Option<AgentInstanceRecord>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<AgentInstanceRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            Ok(agent_instances::table
                .filter(agent_instances::agent_id.eq(agent_id))
                .select(AgentInstanceRow::as_select())
                .first(&mut conn)
                .optional()?
                .map(AgentInstanceRow::into_record))
        })
        .await?
    }

    async fn list_running(&self) -> Result<Vec<AgentInstanceRecord>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentInstanceRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agent_instances::table
                .filter(agent_instances::terminated_at.is_null())
                .select(AgentInstanceRow::as_select())
                .load(&mut conn)
                .context("list running agent_instances")?;
            Ok(rows
                .into_iter()
                .map(AgentInstanceRow::into_record)
                .collect())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn instance(agent_id: &str, handle: &str) -> AgentInstanceRecord {
        AgentInstanceRecord {
            agent_id: agent_id.to_string(),
            backend: "local".to_string(),
            target_type: "local".to_string(),
            handle_id: handle.to_string(),
            pid: Some(4242),
            spawned_at: 1_000,
            backend_config: None,
            terminated_at: None,
            terminated_reason: None,
        }
    }

    #[tokio::test]
    async fn backend_config_roundtrips() {
        // review task-273: kill-reconstruction details (container runtime,
        // k8s namespace/context/kubectl path) must survive the store —
        // the orphan sweep reconstructs the kill target from them.
        let (_tmp, s) = setup();
        let mut rec = instance("agent-1", "pod-a");
        rec.backend = "kubernetes".to_string();
        rec.target_type = "kubernetes".to_string();
        rec.backend_config =
            Some(serde_json::json!({"namespace": "gyre-agents", "context": "prod"}).to_string());
        s.record_spawned(&rec).await.unwrap();
        let found = s.find_by_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(found.backend_config, rec.backend_config);
        // And it is listed for the sweep.
        assert_eq!(
            s.list_running().await.unwrap()[0].backend_config,
            rec.backend_config
        );
    }

    #[tokio::test]
    async fn record_find_list_running_roundtrip() {
        let (_tmp, s) = setup();
        s.record_spawned(&instance("agent-1", "proc-1"))
            .await
            .unwrap();
        s.record_spawned(&instance("agent-2", "proc-2"))
            .await
            .unwrap();

        let found = s
            .find_by_agent("agent-1")
            .await
            .unwrap()
            .expect("row must exist");
        assert_eq!(found.backend, "local");
        assert_eq!(found.handle_id, "proc-1");
        assert_eq!(found.pid, Some(4242));
        assert!(found.terminated_at.is_none());

        let running = s.list_running().await.unwrap();
        assert_eq!(running.len(), 2);
    }

    #[tokio::test]
    async fn record_spawned_replaces_on_respawn() {
        let (_tmp, s) = setup();
        s.record_spawned(&instance("agent-1", "proc-1"))
            .await
            .unwrap();
        // One live instance per agent: a respawn replaces the row.
        s.record_spawned(&instance("agent-1", "proc-1b"))
            .await
            .unwrap();

        let running = s.list_running().await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].handle_id, "proc-1b");
    }

    #[tokio::test]
    async fn mark_terminated_sets_fields_and_fails_for_unknown_agent() {
        let (_tmp, s) = setup();
        s.record_spawned(&instance("agent-1", "proc-1"))
            .await
            .unwrap();

        // Port contract: terminating an unknown agent's instance fails.
        assert!(s
            .mark_terminated("agent-missing", "clean_exit")
            .await
            .is_err());

        s.mark_terminated("agent-1", "remote_agent_cleanup")
            .await
            .unwrap();
        let record = s.find_by_agent("agent-1").await.unwrap().unwrap();
        assert!(record.terminated_at.is_some());
        assert_eq!(
            record.terminated_reason.as_deref(),
            Some("remote_agent_cleanup")
        );
        // Terminated instances leave the sweep candidate set.
        assert!(s.list_running().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn running_records_survive_storage_rebuild_restart_durability() {
        // The orphan-sweep restart scenario: the durable store still lists
        // the instance as running after a storage rebuild (server restart).
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let s = SqliteStorage::new(&path).unwrap();
            s.record_spawned(&instance("agent-orphan", "proc-9"))
                .await
                .unwrap();
        }
        let s2 = SqliteStorage::new(&path).unwrap();
        let running = s2.list_running().await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].agent_id, "agent-orphan");
    }
}
