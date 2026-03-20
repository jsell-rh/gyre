-- M15.3: Add tenant_id to all tenant-scoped tables.
-- Existing rows default to 'default' tenant for backward compatibility.
-- Security: queries must filter by tenant_id; 'system' tenant requires Admin role.

ALTER TABLE projects ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE repositories ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE agents ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE tasks ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE merge_requests ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE activity_events ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE analytics_events ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE cost_entries ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';

-- Tenant-scoped indexes for efficient per-tenant queries
CREATE INDEX IF NOT EXISTS idx_projects_tenant ON projects(tenant_id);
CREATE INDEX IF NOT EXISTS idx_repositories_tenant ON repositories(tenant_id);
CREATE INDEX IF NOT EXISTS idx_agents_tenant ON agents(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tasks_tenant ON tasks(tenant_id);
CREATE INDEX IF NOT EXISTS idx_mr_tenant ON merge_requests(tenant_id);
CREATE INDEX IF NOT EXISTS idx_activity_tenant ON activity_events(tenant_id);
CREATE INDEX IF NOT EXISTS idx_analytics_tenant ON analytics_events(tenant_id);
CREATE INDEX IF NOT EXISTS idx_cost_tenant ON cost_entries(tenant_id);
