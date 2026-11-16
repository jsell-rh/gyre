-- task-146 (analytics.md §Event Schema): scope columns on analytics_events
-- for spec-required filtering by user/workspace/repo and session correlation.
-- NULL = not applicable (platform-internal events may lack a human user).
ALTER TABLE analytics_events ADD COLUMN user_id TEXT;
ALTER TABLE analytics_events ADD COLUMN session_id TEXT;
ALTER TABLE analytics_events ADD COLUMN workspace_id TEXT;
ALTER TABLE analytics_events ADD COLUMN repo_id TEXT;

CREATE INDEX IF NOT EXISTS idx_analytics_user ON analytics_events(user_id);
CREATE INDEX IF NOT EXISTS idx_analytics_workspace ON analytics_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_analytics_repo ON analytics_events(repo_id);
