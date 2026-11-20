-- Revert task-258: drop agent-session tracking.

DROP TABLE agent_sessions;
ALTER TABLE agent_commits DROP COLUMN session_id;
ALTER TABLE agent_commits DROP COLUMN files_changed;
