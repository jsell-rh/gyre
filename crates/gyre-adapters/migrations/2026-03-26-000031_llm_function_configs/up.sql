CREATE TABLE IF NOT EXISTS llm_function_configs (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT,
    function_key  TEXT NOT NULL,
    model_name    TEXT NOT NULL,
    max_tokens    INTEGER,
    updated_by    TEXT NOT NULL,
    updated_at    INTEGER NOT NULL,
    UNIQUE (workspace_id, function_key)
);

CREATE INDEX IF NOT EXISTS idx_llm_function_configs_workspace
    ON llm_function_configs (workspace_id, function_key);
