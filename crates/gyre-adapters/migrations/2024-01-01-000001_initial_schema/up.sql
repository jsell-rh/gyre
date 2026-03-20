-- M15 Diesel initial schema: all tables from migrations 001-008 combined.
-- WAL mode and foreign keys are enabled by the connection customizer.

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at BIGINT NOT NULL,
    is_mirror INTEGER NOT NULL DEFAULT 0,
    mirror_url TEXT,
    mirror_interval_secs BIGINT,
    last_mirror_sync BIGINT
);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'Idle',
    parent_id TEXT REFERENCES agents(id),
    current_task_id TEXT,
    lifetime_budget_secs BIGINT,
    spawned_at BIGINT NOT NULL,
    last_heartbeat BIGINT
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'Backlog',
    priority TEXT NOT NULL DEFAULT 'Medium',
    assigned_to TEXT REFERENCES agents(id),
    parent_task_id TEXT REFERENCES tasks(id),
    labels TEXT NOT NULL DEFAULT '[]',
    branch TEXT,
    pr_link TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_requests (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL REFERENCES repositories(id),
    title TEXT NOT NULL,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Open',
    author_agent_id TEXT REFERENCES agents(id),
    reviewers TEXT NOT NULL DEFAULT '[]',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    diff_files_changed BIGINT,
    diff_insertions BIGINT,
    diff_deletions BIGINT,
    has_conflicts INTEGER
);

CREATE TABLE IF NOT EXISTS activity_events (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    description TEXT NOT NULL,
    timestamp BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_comments (
    id TEXT PRIMARY KEY NOT NULL,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    author_agent_id TEXT NOT NULL,
    body TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS reviews (
    id TEXT PRIMARY KEY NOT NULL,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    reviewer_agent_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    body TEXT,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_queue (
    id TEXT PRIMARY KEY NOT NULL,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id),
    priority INTEGER NOT NULL DEFAULT 50,
    status TEXT NOT NULL DEFAULT 'Queued',
    enqueued_at BIGINT NOT NULL,
    processed_at BIGINT,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS agent_commits (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    timestamp BIGINT NOT NULL,
    task_id TEXT,
    ralph_step TEXT,
    spawned_by_user_id TEXT,
    parent_agent_id TEXT,
    model_context TEXT,
    attestation_level TEXT
);

CREATE TABLE IF NOT EXISTS agent_worktrees (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    task_id TEXT,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    external_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT,
    roles TEXT NOT NULL DEFAULT '[]',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    key TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS analytics_events (
    id TEXT PRIMARY KEY NOT NULL,
    event_name TEXT NOT NULL,
    agent_id TEXT,
    properties TEXT NOT NULL DEFAULT '{}',
    timestamp BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS cost_entries (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    task_id TEXT,
    cost_type TEXT NOT NULL,
    amount DOUBLE PRECISION NOT NULL,
    currency TEXT NOT NULL,
    timestamp BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    path TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    pid INTEGER,
    timestamp BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS siem_targets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    target_type TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS network_peers (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    wireguard_pubkey TEXT NOT NULL,
    endpoint TEXT,
    allowed_ips TEXT NOT NULL DEFAULT '[]',
    registered_at BIGINT NOT NULL,
    last_seen BIGINT
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_assigned_to ON tasks(assigned_to);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_agents_status ON agents(status);
CREATE INDEX IF NOT EXISTS idx_agents_name ON agents(name);
CREATE INDEX IF NOT EXISTS idx_activity_timestamp ON activity_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_activity_agent ON activity_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_mr_status ON merge_requests(status);
CREATE INDEX IF NOT EXISTS idx_mr_repo ON merge_requests(repository_id);
CREATE INDEX IF NOT EXISTS idx_repos_project ON repositories(project_id);
CREATE INDEX IF NOT EXISTS idx_review_comments_mr ON review_comments(merge_request_id);
CREATE INDEX IF NOT EXISTS idx_reviews_mr ON reviews(merge_request_id);
CREATE INDEX IF NOT EXISTS idx_mq_status ON merge_queue(status);
CREATE INDEX IF NOT EXISTS idx_mq_priority_enqueued ON merge_queue(priority DESC, enqueued_at ASC);
CREATE INDEX IF NOT EXISTS idx_agent_commits_agent ON agent_commits(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_commits_repo ON agent_commits(repository_id);
CREATE INDEX IF NOT EXISTS idx_agent_commits_sha ON agent_commits(commit_sha);
CREATE INDEX IF NOT EXISTS idx_agent_commits_task ON agent_commits(task_id);
CREATE INDEX IF NOT EXISTS idx_agent_worktrees_agent ON agent_worktrees(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_worktrees_repo ON agent_worktrees(repository_id);
CREATE INDEX IF NOT EXISTS idx_users_external_id ON users(external_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_analytics_timestamp ON analytics_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_analytics_event_name ON analytics_events(event_name);
CREATE INDEX IF NOT EXISTS idx_analytics_agent_id ON analytics_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_cost_timestamp ON cost_entries(timestamp);
CREATE INDEX IF NOT EXISTS idx_cost_agent_id ON cost_entries(agent_id);
CREATE INDEX IF NOT EXISTS idx_cost_task_id ON cost_entries(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_agent_id ON audit_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_events(event_type);
CREATE INDEX IF NOT EXISTS idx_network_peers_agent ON network_peers(agent_id);
