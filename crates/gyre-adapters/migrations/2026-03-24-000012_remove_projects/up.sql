-- M33: Remove Project entity, make workspace_id required on repositories.
--
-- Steps:
-- 1. Migrate orphaned repos: assign workspace_id from their project's workspace,
--    or 'default' if the project had no workspace.
-- 2. Recreate repositories table without project_id, with workspace_id NOT NULL.
-- 3. Drop the projects table.

-- SQLite doesn't support ALTER COLUMN or DROP COLUMN reliably,
-- so we recreate the table.

-- Step 1: Ensure all repos have a workspace_id before we make it NOT NULL.
UPDATE repositories
SET workspace_id = COALESCE(
    (SELECT p.workspace_id FROM projects p WHERE p.id = repositories.project_id),
    'default'
)
WHERE workspace_id IS NULL;

-- Step 2: Recreate repositories without project_id, workspace_id NOT NULL.
CREATE TABLE repositories_new (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at BIGINT NOT NULL,
    is_mirror INTEGER NOT NULL DEFAULT 0,
    mirror_url TEXT,
    mirror_interval_secs BIGINT,
    last_mirror_sync BIGINT,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    workspace_id TEXT NOT NULL DEFAULT 'default'
);

INSERT INTO repositories_new (id, name, path, default_branch, created_at, is_mirror, mirror_url, mirror_interval_secs, last_mirror_sync, tenant_id, workspace_id)
SELECT id, name, path, default_branch, created_at, is_mirror, mirror_url, mirror_interval_secs, last_mirror_sync, tenant_id, COALESCE(workspace_id, 'default')
FROM repositories;

DROP TABLE repositories;
ALTER TABLE repositories_new RENAME TO repositories;

-- Recreate index
CREATE INDEX IF NOT EXISTS idx_repos_workspace ON repositories(workspace_id);

-- Step 3: Drop projects table.
DROP TABLE IF EXISTS projects;
