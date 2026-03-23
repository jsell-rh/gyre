-- M29.1: Diesel tables for platform entities
-- All entities previously stored in-memory only (HashMap in AppState)

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    budget TEXT,                  -- JSON: Option<BudgetConfig>
    max_repos INTEGER,
    max_agents_per_repo INTEGER,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS personas (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    scope TEXT NOT NULL,          -- JSON: PersonaScope (tagged enum)
    system_prompt TEXT NOT NULL,
    capabilities TEXT NOT NULL,   -- JSON: Vec<String>
    protocols TEXT NOT NULL,      -- JSON: Vec<String>
    model TEXT,
    temperature DOUBLE PRECISION,
    max_tokens INTEGER,
    budget TEXT,                  -- JSON: Option<BudgetConfig>
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS teams (
    id TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    member_ids TEXT NOT NULL,     -- JSON: Vec<Id>
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_memberships (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    role TEXT NOT NULL,
    invited_by TEXT NOT NULL,
    accepted INTEGER NOT NULL DEFAULT 0,
    accepted_at BIGINT,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    notification_type TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    entity_type TEXT,
    entity_id TEXT,
    priority TEXT NOT NULL,
    action_url TEXT,
    read INTEGER NOT NULL DEFAULT 0,
    read_at BIGINT,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS policies (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    scope TEXT NOT NULL,          -- "tenant" | "workspace" | "repo"
    scope_id TEXT,
    priority INTEGER NOT NULL,
    effect TEXT NOT NULL,         -- "allow" | "deny"
    conditions TEXT NOT NULL,     -- JSON: Vec<Condition>
    actions TEXT NOT NULL,        -- JSON: Vec<String>
    resource_types TEXT NOT NULL, -- JSON: Vec<String>
    enabled INTEGER NOT NULL DEFAULT 1,
    built_in INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS policy_decisions (
    request_id TEXT NOT NULL PRIMARY KEY,
    subject_id TEXT NOT NULL,
    subject_type TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    decision TEXT NOT NULL,       -- "allow" | "deny"
    matched_policy TEXT,
    evaluated_policies INTEGER NOT NULL,
    evaluation_ms DOUBLE PRECISION NOT NULL,
    evaluated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS spec_approvals (
    id TEXT NOT NULL PRIMARY KEY,
    spec_path TEXT NOT NULL,
    spec_sha TEXT NOT NULL,
    approver_id TEXT NOT NULL,
    signature TEXT,
    approved_at BIGINT NOT NULL,
    revoked_at BIGINT,
    revoked_by TEXT,
    revocation_reason TEXT
);

CREATE TABLE IF NOT EXISTS dependency_edges (
    id TEXT NOT NULL PRIMARY KEY,
    source_repo_id TEXT NOT NULL,
    target_repo_id TEXT NOT NULL,
    dependency_type TEXT NOT NULL,
    source_artifact TEXT NOT NULL,
    target_artifact TEXT NOT NULL,
    version_pinned TEXT,
    version_drift INTEGER,
    detection_method TEXT NOT NULL,
    status TEXT NOT NULL,
    detected_at BIGINT NOT NULL,
    last_verified_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS budget_configs (
    entity_key TEXT NOT NULL PRIMARY KEY, -- "workspace:{id}" or "tenant:{id}"
    max_tokens_per_day BIGINT,
    max_cost_per_day DOUBLE PRECISION,
    max_concurrent_agents INTEGER,
    max_agent_lifetime_secs BIGINT,
    updated_at BIGINT NOT NULL
);
