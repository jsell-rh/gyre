-- Add tenant_id to system view unique index for defense-in-depth.
-- Prevents cross-tenant collisions if workspace_id is ever reused.
DROP INDEX IF EXISTS idx_saved_views_no_dup_system;
CREATE UNIQUE INDEX idx_saved_views_no_dup_system
    ON saved_views(tenant_id, workspace_id, repo_id, name, is_system) WHERE is_system = 1;
