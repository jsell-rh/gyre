use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus, AgentUsage};
use gyre_ports::AgentRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::agents;

fn status_to_str(s: &AgentStatus) -> &'static str {
    match s {
        AgentStatus::Active => "Active",
        AgentStatus::Idle => "Idle",
        AgentStatus::Failed => "Failed",
        AgentStatus::Stopped => "Stopped",
        AgentStatus::Dead => "Dead",
    }
}

fn str_to_status(s: &str) -> Result<AgentStatus> {
    match s {
        "Active" => Ok(AgentStatus::Active),
        "Idle" => Ok(AgentStatus::Idle),
        "Failed" => Ok(AgentStatus::Failed),
        "Stopped" => Ok(AgentStatus::Stopped),
        "Dead" => Ok(AgentStatus::Dead),
        // Map legacy statuses to spec-compliant equivalents.
        "Blocked" | "Error" => Ok(AgentStatus::Failed),
        "Paused" => Ok(AgentStatus::Stopped),
        other => Err(anyhow!("unknown agent status: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = agents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct AgentRow {
    id: String,
    name: String,
    status: String,
    parent_id: Option<String>,
    current_task_id: Option<String>,
    lifetime_budget_secs: Option<i64>,
    spawned_at: i64,
    last_heartbeat: Option<i64>,
    #[allow(dead_code)]
    tenant_id: String,
    spawned_by: Option<String>,
    workspace_id: String,
    #[allow(dead_code)]
    usage_tokens_input: Option<i64>,
    #[allow(dead_code)]
    usage_tokens_output: Option<i64>,
    #[allow(dead_code)]
    usage_cost_usd: Option<f64>,
}

impl AgentRow {
    fn into_agent(self) -> Result<Agent> {
        Ok(Agent {
            id: Id::new(self.id),
            name: self.name,
            status: str_to_status(&self.status)?,
            parent_id: self.parent_id.map(Id::new),
            current_task_id: self.current_task_id.map(Id::new),
            lifetime_budget_secs: self.lifetime_budget_secs.map(|v| v as u64),
            spawned_at: self.spawned_at as u64,
            last_heartbeat: self.last_heartbeat.map(|v| v as u64),
            spawned_by: self.spawned_by,
            disconnected_behavior: Default::default(),
            workspace_id: Id::new(self.workspace_id),
            iteration: 0,
            loop_config: None,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = agents)]
struct NewAgentRow<'a> {
    id: &'a str,
    name: &'a str,
    status: &'a str,
    parent_id: Option<&'a str>,
    current_task_id: Option<&'a str>,
    lifetime_budget_secs: Option<i64>,
    spawned_at: i64,
    last_heartbeat: Option<i64>,
    tenant_id: &'a str,
    spawned_by: Option<&'a str>,
    workspace_id: &'a str,
}

#[async_trait]
impl AgentRepository for PgStorage {
    async fn create(&self, agent: &Agent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let a = agent.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewAgentRow {
                id: a.id.as_str(),
                name: &a.name,
                status: status_to_str(&a.status),
                parent_id: a.parent_id.as_ref().map(|id| id.as_str()),
                current_task_id: a.current_task_id.as_ref().map(|id| id.as_str()),
                lifetime_budget_secs: a.lifetime_budget_secs.map(|v| v as i64),
                spawned_at: a.spawned_at as i64,
                last_heartbeat: a.last_heartbeat.map(|v| v as i64),
                tenant_id: &tenant,
                spawned_by: a.spawned_by.as_deref(),
                workspace_id: a.workspace_id.as_str(),
            };
            diesel::insert_into(agents::table)
                .values(&row)
                .on_conflict(agents::id)
                .do_update()
                .set((
                    agents::name.eq(row.name),
                    agents::status.eq(row.status),
                    agents::parent_id.eq(row.parent_id),
                    agents::current_task_id.eq(row.current_task_id),
                    agents::lifetime_budget_secs.eq(row.lifetime_budget_secs),
                    agents::last_heartbeat.eq(row.last_heartbeat),
                ))
                .execute(&mut *conn)
                .context("insert agent")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Agent>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = agents::table
                .find(id.as_str())
                .filter(agents::tenant_id.eq(&tenant))
                .first::<AgentRow>(&mut *conn)
                .optional()
                .context("find agent by id")?;
            result.map(AgentRow::into_agent).transpose()
        })
        .await?
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>> {
        let pool = Arc::clone(&self.pool);
        let name = name.to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Agent>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = agents::table
                .filter(agents::name.eq(&name))
                .filter(agents::tenant_id.eq(&tenant))
                .first::<AgentRow>(&mut *conn)
                .optional()
                .context("find agent by name")?;
            result.map(AgentRow::into_agent).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Agent>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Agent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agents::table
                .filter(agents::tenant_id.eq(&tenant))
                .order(agents::spawned_at.asc())
                .load::<AgentRow>(&mut *conn)
                .context("list agents")?;
            rows.into_iter().map(AgentRow::into_agent).collect()
        })
        .await?
    }

    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>> {
        let pool = Arc::clone(&self.pool);
        let status_str = status_to_str(status).to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Agent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agents::table
                .filter(agents::tenant_id.eq(&tenant))
                .filter(agents::status.eq(&status_str))
                .order(agents::spawned_at.asc())
                .load::<AgentRow>(&mut *conn)
                .context("list agents by status")?;
            rows.into_iter().map(AgentRow::into_agent).collect()
        })
        .await?
    }

    async fn update(&self, agent: &Agent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let a = agent.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                agents::table
                    .find(a.id.as_str())
                    .filter(agents::tenant_id.eq(&tenant)),
            )
            .set((
                agents::name.eq(&a.name),
                agents::status.eq(status_to_str(&a.status)),
                agents::parent_id.eq(a.parent_id.as_ref().map(|id| id.as_str())),
                agents::current_task_id.eq(a.current_task_id.as_ref().map(|id| id.as_str())),
                agents::lifetime_budget_secs.eq(a.lifetime_budget_secs.map(|v| v as i64)),
                agents::last_heartbeat.eq(a.last_heartbeat.map(|v| v as i64)),
            ))
            .execute(&mut *conn)
            .context("update agent")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                agents::table
                    .find(id.as_str())
                    .filter(agents::tenant_id.eq(&tenant)),
            )
            .execute(&mut *conn)
            .context("delete agent")?;
            Ok(())
        })
        .await?
    }
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Agent>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Agent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = agents::table
                .filter(agents::tenant_id.eq(&tenant))
                .filter(agents::workspace_id.eq(workspace_id.as_str()))
                .order(agents::spawned_at.asc())
                .load::<AgentRow>(&mut *conn)
                .context("list agents by workspace")?;
            rows.into_iter().map(|r| r.into_agent()).collect()
        })
        .await?
    }

    async fn update_status(&self, agent_id: &Id, status: AgentStatus) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        let status_str = status_to_str(&status).to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                agents::table
                    .find(agent_id.as_str())
                    .filter(agents::tenant_id.eq(&tenant)),
            )
            .set(agents::status.eq(&status_str))
            .execute(&mut *conn)
            .context("update agent status")?;
            Ok(())
        })
        .await?
    }

    async fn record_usage(&self, _usage: &AgentUsage) -> Result<()> {
        // Postgres adapter: usage columns not yet added to PG schema; no-op for now.
        Ok(())
    }
}
