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
    /// Task this commit is associated with.
    pub task_id: Option<String>,
    /// User who spawned the agent that made this commit.
    pub spawned_by_user_id: Option<String>,
    /// Parent agent that spawned this agent.
    pub parent_agent_id: Option<String>,
    /// JSON-encoded model context at time of commit.
    pub model_context: Option<String>,
    /// Attestation level for this commit (M14.2).
    /// Values: "unattested", "self-reported", "server-verified".
    pub attestation_level: Option<String>,
    /// The agent session (context window) that produced this commit
    /// (source-control.md §Agent-to-Commit Tracking — session-to-commit
    /// mapping). `None` when the pushing agent had no open session:
    /// a missing session is recorded as NULL, never a fabricated id.
    pub session_id: Option<String>,
    /// Files changed by this commit, recorded at push time
    /// (`git diff-tree --no-commit-id --name-only -r <sha>`).
    pub files_changed: Vec<String>,
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
            spawned_by_user_id: None,
            parent_agent_id: None,
            model_context: None,
            attestation_level: None,
            session_id: None,
            files_changed: Vec::new(),
        }
    }

    /// Builder-style method to attach provenance metadata.
    pub fn with_provenance(
        mut self,
        task_id: Option<String>,
        spawned_by_user_id: Option<String>,
        parent_agent_id: Option<String>,
        model_context: Option<String>,
    ) -> Self {
        self.task_id = task_id;
        self.spawned_by_user_id = spawned_by_user_id;
        self.parent_agent_id = parent_agent_id;
        self.model_context = model_context;
        self
    }

    /// Set the attestation level for this commit.
    pub fn with_attestation_level(mut self, level: impl Into<String>) -> Self {
        self.attestation_level = Some(level.into());
        self
    }

    /// Stamp the session (context window) that produced this commit.
    /// `None` is recorded as NULL — never a fabricated session id.
    pub fn with_session_id(mut self, session_id: Option<String>) -> Self {
        self.session_id = session_id;
        self
    }

    /// Attach the files changed by this commit (recorded at push time).
    pub fn with_files_changed(mut self, files_changed: Vec<String>) -> Self {
        self.files_changed = files_changed;
        self
    }
}

/// One ephemeral execution of an agent (one context window, one fresh start).
///
/// A Ralph loop is one agent running multiple sessions until convergence.
/// Sessions track execution metadata; the provenance chain tracks code
/// attribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Id,
    pub agent_id: Id,
    /// Repository the agent worked on during this session.
    pub repository_id: Id,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Branch checked out at session start (the spawn-time branch).
    pub initial_branch: String,
    /// Branch resolved with a real worktree lookup at session end.
    /// `None` until termination, or when the worktree could not be read.
    pub final_branch: Option<String>,
    /// Whether the agent left uncommitted work at termination.
    /// `None` = not yet terminated, or the worktree state could not be
    /// determined (missing/inaccessible worktree records an explicit
    /// unknown state — never a silent clean).
    pub dirty: Option<bool>,
    /// Uncommitted paths detected at termination (empty unless dirty).
    pub uncommitted_paths: Vec<String>,
    /// Why the dirty state could not be determined (missing worktree, VCS
    /// query failure). Populated only when `dirty` is `None` after
    /// termination.
    pub dirty_detail: Option<String>,
    /// Total cost in USD for this session.
    pub cost: Option<f64>,
    /// Total tokens used across input + output.
    pub tokens_used: Option<u64>,
    /// IDs of inbox messages read during this session.
    pub messages_read: Vec<String>,
}

impl Session {
    pub fn new(
        id: Id,
        agent_id: Id,
        repository_id: Id,
        started_at: u64,
        initial_branch: impl Into<String>,
    ) -> Self {
        Self {
            id,
            agent_id,
            repository_id,
            started_at,
            ended_at: None,
            initial_branch: initial_branch.into(),
            final_branch: None,
            dirty: None,
            uncommitted_paths: Vec::new(),
            dirty_detail: None,
            cost: None,
            tokens_used: None,
            messages_read: Vec::new(),
        }
    }
}

/// Configuration for the Ralph loop terminal conditions.
///
/// When present on a spawn request, the server manages the session cycle automatically.
/// When absent, the agent runs a single session (backward-compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Enable agent review after gates pass (default: true).
    pub agent_review: bool,
    /// Persona ID to use for the reviewer agent.
    pub reviewer_persona_id: Option<String>,
    /// Maximum number of sessions before escalating to orchestrator.
    pub max_iterations: u32,
    /// Maximum number of reviewer rejections before escalating.
    pub max_review_rejections: u32,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            agent_review: true,
            reviewer_persona_id: None,
            max_iterations: 50,
            max_review_rejections: 5,
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
        assert!(ac.task_id.is_none());
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
        .with_provenance(Some("TASK-001".to_string()), None, None, None);
        assert_eq!(ac.task_id.as_deref(), Some("TASK-001"));
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

    #[test]
    fn test_session_new() {
        let s = Session::new(
            Id::new("s1"),
            Id::new("agent1"),
            Id::new("repo1"),
            1000,
            "feat/x",
        );
        assert_eq!(s.agent_id, Id::new("agent1"));
        assert_eq!(s.repository_id, Id::new("repo1"));
        assert_eq!(s.started_at, 1000);
        assert_eq!(s.initial_branch, "feat/x");
        assert!(s.ended_at.is_none());
        assert!(s.final_branch.is_none());
        assert!(s.dirty.is_none());
        assert!(s.uncommitted_paths.is_empty());
        assert!(s.dirty_detail.is_none());
        assert!(s.messages_read.is_empty());
    }

    #[test]
    fn test_agent_commit_session_and_files() {
        let ac = AgentCommit::new(
            Id::new("c3"),
            Id::new("agent1"),
            Id::new("repo1"),
            "sha789",
            "main",
            3000,
        )
        .with_session_id(Some("s1".to_string()))
        .with_files_changed(vec!["src/lib.rs".to_string()]);
        assert_eq!(ac.session_id.as_deref(), Some("s1"));
        assert_eq!(ac.files_changed, vec!["src/lib.rs".to_string()]);

        // A missing session stays NULL — never a fabricated id.
        let bare = AgentCommit::new(
            Id::new("c4"),
            Id::new("agent1"),
            Id::new("repo1"),
            "sha000",
            "main",
            3001,
        );
        assert!(bare.session_id.is_none());
        assert!(bare.files_changed.is_empty());
    }

    #[test]
    fn test_loop_config_default() {
        let lc = LoopConfig::default();
        assert!(lc.agent_review);
        assert_eq!(lc.max_iterations, 50);
        assert_eq!(lc.max_review_rejections, 5);
    }
}
