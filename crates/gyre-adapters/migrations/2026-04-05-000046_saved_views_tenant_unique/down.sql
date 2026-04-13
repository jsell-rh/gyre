-- Revert to previous unique index without tenant_id.
DROP INDEX IF EXISTS idx_saved_views_no_dup_system;
CREATE UNIQUE INDEX idx_saved_views_no_dup_system
    ON saved_views(workspace_id, repo_id, name, is_system) WHERE is_system = 1;
