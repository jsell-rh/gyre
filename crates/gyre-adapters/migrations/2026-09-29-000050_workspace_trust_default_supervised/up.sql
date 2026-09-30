-- TASK-077: Default trust level is Supervised (HSI §2: "A new experimental
-- workspace starts at Supervised"). Migration 000024 set the DB default to
-- 'Guided', which contradicted the spec.
--
-- SQLite doesn't support ALTER COLUMN SET DEFAULT, so the table is rebuilt
-- (same pattern as 000013). Existing rows keep their current trust level —
-- this migration only changes the DEFAULT for future inserts.
CREATE TABLE workspaces_new (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    budget TEXT,
    max_repos INTEGER,
    max_agents_per_repo INTEGER,
    trust_level TEXT NOT NULL DEFAULT 'Supervised',
    llm_model TEXT,
    created_at BIGINT NOT NULL,
    compute_target_id TEXT,
    UNIQUE (tenant_id, slug)
);

INSERT INTO workspaces_new
    (id, tenant_id, name, slug, description, budget, max_repos,
     max_agents_per_repo, trust_level, llm_model, created_at, compute_target_id)
SELECT id, tenant_id, name, slug, description, budget, max_repos,
       max_agents_per_repo, trust_level, llm_model, created_at, compute_target_id
FROM workspaces;

DROP TABLE workspaces;
ALTER TABLE workspaces_new RENAME TO workspaces;

-- compute_target_id previously carried a REFERENCES compute_targets(id)
-- constraint (added 000033); SQLite table rebuilds drop FK constraints, and
-- no migration after 000033 re-created it, so none is re-created here either.
