-- Revert the workspaces rebuild from 000050 (default back to 'Guided').
CREATE TABLE workspaces_old (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    budget TEXT,
    max_repos INTEGER,
    max_agents_per_repo INTEGER,
    trust_level TEXT NOT NULL DEFAULT 'Guided',
    llm_model TEXT,
    created_at BIGINT NOT NULL,
    compute_target_id TEXT,
    UNIQUE (tenant_id, slug)
);

INSERT INTO workspaces_old
    (id, tenant_id, name, slug, description, budget, max_repos,
     max_agents_per_repo, trust_level, llm_model, created_at, compute_target_id)
SELECT id, tenant_id, name, slug, description, budget, max_repos,
       max_agents_per_repo, trust_level, llm_model, created_at, compute_target_id
FROM workspaces;

DROP TABLE workspaces;
ALTER TABLE workspaces_old RENAME TO workspaces;
