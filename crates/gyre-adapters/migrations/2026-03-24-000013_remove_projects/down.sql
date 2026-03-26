-- Reverse M33: re-add projects table and project_id column on repositories.
-- Note: project data is lost; this creates a 'default' project for all repos.

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    workspace_id TEXT
);

-- Insert a default project for rollback.
INSERT OR IGNORE INTO projects (id, name, created_at, updated_at, tenant_id)
VALUES ('default', 'default', 0, 0, 'default');

-- Re-add project_id to repositories.
CREATE TABLE repositories_old (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL DEFAULT 'default' REFERENCES projects(id),
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at BIGINT NOT NULL,
    is_mirror INTEGER NOT NULL DEFAULT 0,
    mirror_url TEXT,
    mirror_interval_secs BIGINT,
    last_mirror_sync BIGINT,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    workspace_id TEXT
);

INSERT INTO repositories_old (id, project_id, name, path, default_branch, created_at, is_mirror, mirror_url, mirror_interval_secs, last_mirror_sync, tenant_id, workspace_id)
SELECT id, 'default', name, path, default_branch, created_at, is_mirror, mirror_url, mirror_interval_secs, last_mirror_sync, tenant_id, workspace_id
FROM repositories;

DROP TABLE repositories;
ALTER TABLE repositories_old RENAME TO repositories;

CREATE INDEX IF NOT EXISTS idx_repos_project ON repositories(project_id);
CREATE INDEX IF NOT EXISTS idx_repos_workspace ON repositories(workspace_id);
