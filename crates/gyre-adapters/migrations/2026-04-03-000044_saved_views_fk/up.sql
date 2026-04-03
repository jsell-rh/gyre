-- Add ON DELETE CASCADE to saved_views via table rebuild (SQLite limitation).
-- This ensures orphaned saved views are cleaned up when a repository is deleted.

CREATE TABLE saved_views_new (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    query_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_system BOOLEAN NOT NULL DEFAULT FALSE
);

INSERT INTO saved_views_new SELECT * FROM saved_views;
DROP TABLE saved_views;
ALTER TABLE saved_views_new RENAME TO saved_views;

CREATE INDEX IF NOT EXISTS idx_saved_views_repo ON saved_views(repo_id);
CREATE INDEX IF NOT EXISTS idx_saved_views_workspace ON saved_views(workspace_id);
