-- M34 Slice 6: Add unique constraint on (tenant_id, slug) for workspaces.
-- Required for git URL resolution: /git/:workspace_slug/:repo_name/* must be unambiguous.
--
-- SQLite does not support ADD CONSTRAINT on existing tables, so we recreate the table
-- and copy data.

CREATE TABLE workspaces_new (
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

INSERT INTO workspaces_new
    SELECT id, tenant_id, name, slug, description, budget, max_repos, max_agents_per_repo, created_at
    FROM workspaces;

DROP TABLE workspaces;

ALTER TABLE workspaces_new RENAME TO workspaces;
