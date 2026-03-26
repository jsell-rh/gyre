-- Reverse: remove unique constraint on (tenant_id, slug).
CREATE TABLE workspaces_old (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    budget TEXT,
    max_repos INTEGER,
    max_agents_per_repo INTEGER,
    created_at BIGINT NOT NULL
);

INSERT INTO workspaces_old
    SELECT id, tenant_id, name, slug, description, budget, max_repos, max_agents_per_repo, created_at
    FROM workspaces;

DROP TABLE workspaces;

ALTER TABLE workspaces_old RENAME TO workspaces;
