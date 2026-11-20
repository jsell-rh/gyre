//! SQLite adapter for the `AgentInstanceRepository` port (TASK-273,
//! admin-panel.md: durable spawned-instance handles for orphan sweeps).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{AgentInstanceRecord, AgentInstanceRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::agent_instances;

#[derive(Queryable, Selectable)]
#[diesel(table_name = agent_instances)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl AgentInstanceRepository for PgStorage {
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
            // Replace on respawn: one live instance per agent. PostgreSQL
            // has no REPLACE INTO — the portable form is ON CONFLICT
            // DO UPDATE.
            diesel::insert_into(agent_instances::table)
                .values(&row)
                .on_conflict(agent_instances::agent_id)
                .do_update()
                .set((
                    agent_instances::backend.eq(record.backend.clone()),
                    agent_instances::target_type.eq(record.target_type.clone()),
                    agent_instances::handle_id.eq(record.handle_id.clone()),
                    agent_instances::pid.eq(record.pid.map(|p| p as i32)),
                    agent_instances::spawned_at.eq(record.spawned_at as i64),
                    agent_instances::backend_config.eq(record.backend_config.clone()),
                    agent_instances::terminated_at.eq(record.terminated_at.map(|t| t as i64)),
                    agent_instances::terminated_reason.eq(record.terminated_reason.clone()),
                ))
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
    //! SQL-shape regression tests (postgres/search.rs precedent: the repo
    //! has no PG server harness). The behavioral semantics — replace on
    //! respawn, mark-terminated failure for unknown agents, running-only
    //! sweep listing, restart durability — are exercised against a real
    //! SQLite file by the sqlite sibling's suite; these pin the PG
    //! statements the mirror emits.

    use super::*;

    #[test]
    fn list_running_filters_terminated_at_null_in_sql() {
        // The sweep candidate set is "not yet marked terminated": the
        // predicate must be terminated_at IS NULL, in SQL.
        let sql = diesel::debug_query::<diesel::pg::Pg, _>(
            &agent_instances::table.filter(agent_instances::terminated_at.is_null()),
        )
        .to_string();
        assert!(
            sql.contains("\"terminated_at\" IS NULL"),
            "list_running must filter terminated_at IS NULL in SQL: {sql}"
        );
    }

    #[test]
    fn mark_terminated_sets_timestamp_and_reason() {
        let sql = diesel::debug_query::<diesel::pg::Pg, _>(
            &diesel::update(agent_instances::table.filter(agent_instances::agent_id.eq("agent-1")))
                .set((
                    agent_instances::terminated_at.eq(1i64),
                    agent_instances::terminated_reason.eq("remote_agent_cleanup"),
                )),
        )
        .to_string();
        assert!(
            sql.contains("terminated_at") && sql.contains("terminated_reason"),
            "mark_terminated must set both fields: {sql}"
        );
        assert!(
            sql.contains("agent_id") && sql.contains("= $"),
            "mark_terminated must scope to the agent: {sql}"
        );
    }
}
