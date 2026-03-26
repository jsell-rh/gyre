-- Revert HSI S1.2: Platform Model Amendments
-- SQLite does not support DROP COLUMN before 3.35; recreate workspaces table.

DROP INDEX IF EXISTS idx_budget_call_records_workspace;
DROP INDEX IF EXISTS idx_budget_call_records_tenant;
DROP TABLE IF EXISTS budget_call_records;

DROP INDEX IF EXISTS idx_repositories_workspace_name;

-- Recreate workspaces table without trust_level and llm_model columns.
CREATE TABLE workspaces_revert (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    budget TEXT,
    max_repos INTEGER,
    max_agents_per_repo INTEGER,
    created_at BIGINT NOT NULL,
    UNIQUE (tenant_id, slug)
);

INSERT INTO workspaces_revert
    SELECT id, tenant_id, name, slug, description, budget,
           max_repos, max_agents_per_repo, created_at
    FROM workspaces;

DROP TABLE workspaces;
ALTER TABLE workspaces_revert RENAME TO workspaces;
