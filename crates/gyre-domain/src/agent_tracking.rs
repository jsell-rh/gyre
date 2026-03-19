use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Records which agent authored a specific git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommit {
    pub id: Id,
    pub agent_id: Id,
    pub repository_id: Id,
    pub commit_sha: String,
    pub branch: String,
    pub timestamp: u64,
}

impl AgentCommit {
    pub fn new(
        id: Id,
        agent_id: Id,
        repository_id: Id,
        commit_sha: impl Into<String>,
        branch: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            agent_id,
            repository_id,
            commit_sha: commit_sha.into(),
            branch: branch.into(),
            timestamp,
        }
    }
}

/// Tracks a git worktree checked out for a specific agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorktree {
    pub id: Id,
    pub agent_id: Id,
    pub repository_id: Id,
    pub task_id: Option<Id>,
    pub branch: String,
    pub path: String,
    pub created_at: u64,
}

impl AgentWorktree {
    pub fn new(
        id: Id,
        agent_id: Id,
        repository_id: Id,
        task_id: Option<Id>,
        branch: impl Into<String>,
        path: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            agent_id,
            repository_id,
            task_id,
            branch: branch.into(),
            path: path.into(),
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_commit_new() {
        let ac = AgentCommit::new(
            Id::new("c1"),
            Id::new("agent1"),
            Id::new("repo1"),
            "abc123",
            "main",
            1000,
        );
        assert_eq!(ac.commit_sha, "abc123");
        assert_eq!(ac.branch, "main");
        assert_eq!(ac.agent_id, Id::new("agent1"));
    }

    #[test]
    fn test_agent_worktree_new() {
        let wt = AgentWorktree::new(
            Id::new("wt1"),
            Id::new("agent1"),
            Id::new("repo1"),
            Some(Id::new("task1")),
            "feat/x",
            "/tmp/worktrees/agent1",
            2000,
        );
        assert_eq!(wt.branch, "feat/x");
        assert_eq!(wt.path, "/tmp/worktrees/agent1");
        assert!(wt.task_id.is_some());
    }

    #[test]
    fn test_agent_worktree_no_task() {
        let wt = AgentWorktree::new(
            Id::new("wt2"),
            Id::new("agent2"),
            Id::new("repo1"),
            None,
            "main",
            "/tmp/worktrees/agent2",
            3000,
        );
        assert!(wt.task_id.is_none());
    }
}
