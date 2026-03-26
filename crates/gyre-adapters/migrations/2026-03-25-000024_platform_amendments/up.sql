-- HSI S1.2: Platform Model Amendments
-- 1. Workspace trust level + LLM model selection
-- 2. Repository unique constraint (workspace_id, name)
-- 3. Budget call records table (per-call LLM audit log)

-- Workspace: add trust_level (default Guided per spec) and llm_model.
ALTER TABLE workspaces ADD COLUMN trust_level TEXT NOT NULL DEFAULT 'Guided';
ALTER TABLE workspaces ADD COLUMN llm_model TEXT;

-- Repository: unique (workspace_id, name) constraint for cross-workspace spec link resolution.
CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_workspace_name
    ON repositories (workspace_id, name);

-- Budget call records: per-call LLM audit log (append-only).
-- repo_id and agent_id are nullable for user-initiated queries (briefing, explorer, spec assist).
CREATE TABLE IF NOT EXISTS budget_call_records (
    id            TEXT    NOT NULL PRIMARY KEY,
    tenant_id     TEXT    NOT NULL,
    workspace_id  TEXT    NOT NULL,
    repo_id       TEXT,
    agent_id      TEXT,
    task_id       TEXT,
    usage_type    TEXT    NOT NULL,
    input_tokens  BIGINT  NOT NULL DEFAULT 0,
    output_tokens BIGINT  NOT NULL DEFAULT 0,
    cost_usd      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    model         TEXT    NOT NULL,
    timestamp     BIGINT  NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_budget_call_records_tenant
    ON budget_call_records (tenant_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_budget_call_records_workspace
    ON budget_call_records (workspace_id, timestamp DESC);
