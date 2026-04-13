-- Rebuild saved_views without FK constraint (repo_id may be "__workspace__"
-- for workspace-scoped views) and add a UNIQUE constraint to prevent
-- duplicate system views from seeding races.

CREATE TABLE saved_views_new (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL,
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_views_no_dup_system
    ON saved_views(repo_id, name, is_system) WHERE is_system = 1;
