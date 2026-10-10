DROP INDEX IF EXISTS idx_analytics_repo;
DROP INDEX IF EXISTS idx_analytics_workspace;
DROP INDEX IF EXISTS idx_analytics_user;
ALTER TABLE analytics_events DROP COLUMN repo_id;
ALTER TABLE analytics_events DROP COLUMN workspace_id;
ALTER TABLE analytics_events DROP COLUMN session_id;
ALTER TABLE analytics_events DROP COLUMN user_id;
