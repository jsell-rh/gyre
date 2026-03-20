use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a step in the Ralph autonomous development loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RalphStep {
    Spec,
    Implement,
    Review,
    Merge,
}

impl fmt::Display for RalphStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RalphStep {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "spec" => Some(RalphStep::Spec),
            "implement" => Some(RalphStep::Implement),
            "review" => Some(RalphStep::Review),
            "merge" => Some(RalphStep::Merge),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RalphStep::Spec => "spec",
            RalphStep::Implement => "implement",
            RalphStep::Review => "review",
            RalphStep::Merge => "merge",
        }
    }
}

/// Records which agent authored a specific git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommit {
    pub id: Id,
    pub agent_id: Id,
    pub repository_id: Id,
    pub commit_sha: String,
    pub branch: String,
    pub timestamp: u64,
    /// Task this commit is associated with.
    pub task_id: Option<String>,
    /// Which step of the Ralph loop produced this commit.
    pub ralph_step: Option<RalphStep>,
    /// User who spawned the agent that made this commit.
    pub spawned_by_user_id: Option<String>,
    /// Parent agent that spawned this agent.
    pub parent_agent_id: Option<String>,
    /// JSON-encoded model context at time of commit.
    pub model_context: Option<String>,
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
            task_id: None,
            ralph_step: None,
            spawned_by_user_id: None,
            parent_agent_id: None,
            model_context: None,
        }
    }

    /// Builder-style method to attach provenance metadata.
    pub fn with_provenance(
        mut self,
        task_id: Option<String>,
        ralph_step: Option<RalphStep>,
        spawned_by_user_id: Option<String>,
        parent_agent_id: Option<String>,
        model_context: Option<String>,
    ) -> Self {
        self.task_id = task_id;
        self.ralph_step = ralph_step;
        self.spawned_by_user_id = spawned_by_user_id;
        self.parent_agent_id = parent_agent_id;
        self.model_context = model_context;
        self
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
        assert!(ac.task_id.is_none());
        assert!(ac.ralph_step.is_none());
    }

    #[test]
    fn test_agent_commit_with_provenance() {
        let ac = AgentCommit::new(
            Id::new("c2"),
            Id::new("agent1"),
            Id::new("repo1"),
            "def456",
            "feat/x",
            2000,
        )
        .with_provenance(
            Some("TASK-001".to_string()),
            Some(RalphStep::Implement),
            None,
            None,
            None,
        );
        assert_eq!(ac.task_id.as_deref(), Some("TASK-001"));
        assert_eq!(ac.ralph_step, Some(RalphStep::Implement));
    }

    #[test]
    fn test_ralph_step_display() {
        assert_eq!(RalphStep::Spec.to_string(), "spec");
        assert_eq!(RalphStep::Implement.to_string(), "implement");
        assert_eq!(RalphStep::Review.to_string(), "review");
        assert_eq!(RalphStep::Merge.to_string(), "merge");
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
