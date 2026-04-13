-- Fix unique index on system views to include workspace_id.
-- Without workspace_id, workspace-scoped system views (repo_id="__workspace__")
-- from the first workspace block subsequent workspaces via INSERT OR IGNORE.
DROP INDEX IF EXISTS idx_saved_views_no_dup_system;
CREATE UNIQUE INDEX idx_saved_views_no_dup_system
    ON saved_views(workspace_id, repo_id, name, is_system) WHERE is_system = 1;
