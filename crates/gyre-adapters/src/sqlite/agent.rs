use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus, AgentUsage};
use gyre_ports::AgentRepository;
use std::sync::Arc;

use super::SqliteStorage;
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
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl AgentRepository for SqliteStorage {
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
                    agents::spawned_by.eq(row.spawned_by),
                    agents::workspace_id.eq(row.workspace_id),
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

    async fn record_usage(&self, usage: &AgentUsage) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let agent_id = usage.agent_id.clone();
        let tokens_input = usage.tokens_input as i64;
        let tokens_output = usage.tokens_output as i64;
        let cost_usd = usage.cost_usd;
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Read current values and sum them.
            let current: Option<(Option<i64>, Option<i64>, Option<f64>)> = agents::table
                .find(agent_id.as_str())
                .filter(agents::tenant_id.eq(&tenant))
                .select((
                    agents::usage_tokens_input,
                    agents::usage_tokens_output,
                    agents::usage_cost_usd,
                ))
                .first(&mut *conn)
                .optional()
                .context("fetch current usage")?;

            let (new_input, new_output, new_cost) = match current {
                Some((prev_in, prev_out, prev_cost)) => (
                    prev_in.unwrap_or(0) + tokens_input,
                    prev_out.unwrap_or(0) + tokens_output,
                    prev_cost.unwrap_or(0.0) + cost_usd,
                ),
                None => (tokens_input, tokens_output, cost_usd),
            };

            diesel::update(
                agents::table
                    .find(agent_id.as_str())
                    .filter(agents::tenant_id.eq(&tenant)),
            )
            .set((
                agents::usage_tokens_input.eq(new_input),
                agents::usage_tokens_output.eq(new_output),
                agents::usage_cost_usd.eq(new_cost),
            ))
            .execute(&mut *conn)
            .context("record agent usage")?;
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

    #[tokio::test]
    async fn tenant_isolation() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let t1 = SqliteStorage::new_for_tenant(path, "t1").unwrap();
        let t2 = SqliteStorage::new_for_tenant(path, "t2").unwrap();

        t1.create(&make_agent("a1", "agent-t1")).await.unwrap();
        t2.create(&make_agent("a2", "agent-t2")).await.unwrap();

        assert_eq!(t1.list().await.unwrap().len(), 1);
        assert_eq!(t2.list().await.unwrap().len(), 1);
        assert!(t1.find_by_id(&Id::new("a2")).await.unwrap().is_none());
        assert!(t2.find_by_id(&Id::new("a1")).await.unwrap().is_none());
    }
}
