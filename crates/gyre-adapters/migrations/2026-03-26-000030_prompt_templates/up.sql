CREATE TABLE IF NOT EXISTS prompt_templates (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT,               -- NULL = tenant default
    function_key  TEXT NOT NULL,      -- "graph-predict" etc.
    content       TEXT NOT NULL,
    created_by    TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    UNIQUE (workspace_id, function_key)  -- one override per function per workspace
);

CREATE INDEX IF NOT EXISTS idx_prompt_templates_workspace
    ON prompt_templates (workspace_id, function_key);
