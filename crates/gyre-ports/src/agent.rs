use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Agent, AgentStatus, AgentUsage};

#[async_trait]
pub trait AgentRepository: Send + Sync {
    async fn create(&self, agent: &Agent) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>>;
    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>>;
    async fn list(&self) -> Result<Vec<Agent>>;
    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>>;
    async fn update(&self, agent: &Agent) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Agent>>;
    /// Update only the status column for an agent (optimized path vs full update).
    async fn update_status(&self, agent_id: &Id, status: AgentStatus) -> Result<()>;
    /// Record token/cost usage for an agent session; adds to any existing usage columns.
    async fn record_usage(&self, usage: &AgentUsage) -> Result<()>;
}
