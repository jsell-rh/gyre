use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Task, TaskStatus};

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create(&self, task: &Task) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>>;
    async fn list(&self) -> Result<Vec<Task>>;
    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>>;
    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>>;
    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>>;
    async fn update(&self, task: &Task) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Task>>;
    async fn list_by_spec_path(&self, spec_path: &str) -> Result<Vec<Task>>;
}
