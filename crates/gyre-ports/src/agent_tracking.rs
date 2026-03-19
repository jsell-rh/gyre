use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AgentCommit, AgentWorktree};

#[async_trait]
pub trait AgentCommitRepository: Send + Sync {
    /// Record a mapping between an agent and a git commit.
    async fn record(&self, mapping: &AgentCommit) -> Result<()>;

    /// Find all commits authored by the given agent.
    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>>;

    /// Find all commits in the given repository.
    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>>;

    /// Find the commit mapping for a specific SHA.
    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>>;
}

#[async_trait]
pub trait WorktreeRepository: Send + Sync {
    /// Register a new worktree.
    async fn create(&self, worktree: &AgentWorktree) -> Result<()>;

    /// Find all worktrees for the given agent.
    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentWorktree>>;

    /// Find all worktrees in the given repository.
    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentWorktree>>;

    /// Delete a worktree record by id.
    async fn delete(&self, id: &Id) -> Result<()>;
}
