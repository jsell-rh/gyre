CREATE TABLE IF NOT EXISTS explorer_views (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    spec_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    is_builtin INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_explorer_views_workspace ON explorer_views(workspace_id);
CREATE INDEX IF NOT EXISTS idx_explorer_views_created_by ON explorer_views(created_by);
