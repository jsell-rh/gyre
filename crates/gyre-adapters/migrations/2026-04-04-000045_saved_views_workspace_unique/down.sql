-- Revert to the original unique index without workspace_id.
DROP INDEX IF EXISTS idx_saved_views_no_dup_system;
CREATE UNIQUE INDEX idx_saved_views_no_dup_system
    ON saved_views(repo_id, name, is_system) WHERE is_system = 1;
