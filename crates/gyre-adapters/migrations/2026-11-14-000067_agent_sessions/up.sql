-- TASK-258: agent-to-commit tracking at session granularity
-- (specs/system/source-control.md §Agent-to-Commit Tracking).
--
-- The Session domain entity (one context window per agent execution —
-- spawn or Ralph-loop respawn with fresh context) is now persisted, and
-- every recorded AgentCommit carries the session that produced it plus
-- the files it changed (recorded at push time), so "which agent session
-- touched which commits / this file" is answerable at session
-- granularity, not just agent granularity.
--
-- agent_sessions.ended_at is NULL while the session is open and holds the
-- Unix second of termination on every termination path (complete, fail,
-- stale Dead). dirty is NULL until termination and stays NULL when the
-- worktree state could not be determined (missing/inaccessible worktree
-- records an explicit unknown state — never a silent clean);
-- dirty_detail names why. uncommitted_paths and messages_read are JSON
-- arrays of strings.
--
-- agent_commits.session_id is NULL when the pushing agent had no open
-- session — never a fabricated id. files_changed is a JSON array of
-- paths (empty when none were recorded).
--
-- Portability: plain CREATE TABLE / ALTER TABLE ADD COLUMN with no
-- dialect-only syntax, runnable by both the SQLite and PostgreSQL
-- embedded-migration runners.

CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    started_at BIGINT NOT NULL,
    ended_at BIGINT,
    initial_branch TEXT NOT NULL,
    final_branch TEXT,
    dirty BOOLEAN,
    uncommitted_paths TEXT NOT NULL DEFAULT '[]',
    dirty_detail TEXT,
    cost DOUBLE PRECISION,
    tokens_used BIGINT,
    messages_read TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_agent_sessions_agent ON agent_sessions(agent_id);
CREATE INDEX idx_agent_sessions_repo ON agent_sessions(repository_id);

ALTER TABLE agent_commits ADD COLUMN session_id TEXT;
ALTER TABLE agent_commits ADD COLUMN files_changed TEXT NOT NULL DEFAULT '[]';
