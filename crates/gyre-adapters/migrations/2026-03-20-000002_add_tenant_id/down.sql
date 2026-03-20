-- M15.3 down: Remove tenant_id from all tenant-scoped tables.
-- SQLite 3.35.0+ supports DROP COLUMN directly.

DROP INDEX IF EXISTS idx_projects_tenant;
DROP INDEX IF EXISTS idx_repositories_tenant;
DROP INDEX IF EXISTS idx_agents_tenant;
DROP INDEX IF EXISTS idx_tasks_tenant;
DROP INDEX IF EXISTS idx_mr_tenant;
DROP INDEX IF EXISTS idx_activity_tenant;
DROP INDEX IF EXISTS idx_analytics_tenant;
DROP INDEX IF EXISTS idx_cost_tenant;

ALTER TABLE projects DROP COLUMN tenant_id;
ALTER TABLE repositories DROP COLUMN tenant_id;
ALTER TABLE agents DROP COLUMN tenant_id;
ALTER TABLE tasks DROP COLUMN tenant_id;
ALTER TABLE merge_requests DROP COLUMN tenant_id;
ALTER TABLE activity_events DROP COLUMN tenant_id;
ALTER TABLE analytics_events DROP COLUMN tenant_id;
ALTER TABLE cost_entries DROP COLUMN tenant_id;
