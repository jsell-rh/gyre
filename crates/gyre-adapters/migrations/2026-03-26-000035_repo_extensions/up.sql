-- Migration 000035: Repo lifecycle extensions
-- Adds description, status, updated_at to repositories table

ALTER TABLE repositories ADD COLUMN description TEXT;
ALTER TABLE repositories ADD COLUMN status TEXT NOT NULL DEFAULT 'Active';
ALTER TABLE repositories ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;

-- Enforce workspace-scoped name uniqueness at the DB level
CREATE UNIQUE INDEX IF NOT EXISTS idx_repos_workspace_name ON repositories (workspace_id, name);
