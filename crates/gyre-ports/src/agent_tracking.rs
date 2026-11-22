use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{AgentCommit, AgentWorktree, Session};

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait AgentCommitRepository: Send + Sync {
    /// Record a mapping between an agent and a git commit.
    async fn record(&self, mapping: &AgentCommit) -> Result<()>;

    /// Find all commits authored by the given agent.
    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>>;

    /// Find all commits in the given repository.
    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>>;

    /// Find the commit mapping for a specific SHA.
    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>>;

    /// Find all commits associated with a given task ID.
    async fn find_by_task(&self, task_id: &str) -> Result<Vec<AgentCommit>>;

    /// Find all commits recorded during the given session, chronological
    /// (oldest first) — source-control.md §Agent-to-Commit Tracking
    /// (session-to-commit mapping).
    async fn find_by_session(&self, session_id: &Id) -> Result<Vec<AgentCommit>>;

    /// Find all commits in the repository whose changed files include
    /// `path` — source-control.md §Agent-to-Commit Tracking
    /// (commit-based queries by file).
    async fn find_by_file(&self, repo_id: &Id, path: &str) -> Result<Vec<AgentCommit>>;
}

/// Persistence for the agent `Session` entity (one context window per
/// execution — source-control.md §Agent-to-Commit Tracking).
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait AgentSessionRepository: Send + Sync {
    /// Start a new session. Fails if a session with the same `id` already
    /// exists.
    async fn create(&self, session: &Session) -> Result<()>;

    /// Find all sessions for the given agent, chronological (oldest first).
    /// Same-`started_at` ties (timestamps have second granularity) break by
    /// session window — an ended session sorts before a still-open session
    /// started in the same second (one agent's sessions are sequential
    /// context windows), then by `ended_at`, then `id` — never by the
    /// random-UUID `id` alone.
    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<Session>>;

    /// Find all sessions for the given repository, chronological (oldest
    /// first), with the same same-second tie-break as [`Self::find_by_agent`].
    async fn find_by_repo(&self, repository_id: &Id) -> Result<Vec<Session>>;

    /// Find a session by id.
    async fn find_by_id(&self, id: &Id) -> Result<Option<Session>>;

    /// The agent's currently-open session (no `ended_at` yet), if any —
    /// the session a pushed commit is attributed to.
    async fn find_open_by_agent(&self, agent_id: &Id) -> Result<Option<Session>>;

    /// Persist session state (termination fields: `ended_at`,
    /// `final_branch`, dirty state). Fails if the session does not exist.
    async fn update(&self, session: &Session) -> Result<()>;
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
