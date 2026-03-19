use anyhow::Result;
use rusqlite::Connection;

const MIGRATION_001: &str = "
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'Idle',
    parent_id TEXT REFERENCES agents(id),
    current_task_id TEXT,
    lifetime_budget_secs INTEGER,
    spawned_at INTEGER NOT NULL,
    last_heartbeat INTEGER
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'Backlog',
    priority TEXT NOT NULL DEFAULT 'Medium',
    assigned_to TEXT REFERENCES agents(id),
    parent_task_id TEXT REFERENCES tasks(id),
    labels TEXT NOT NULL DEFAULT '[]',
    branch TEXT,
    pr_link TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_requests (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id),
    title TEXT NOT NULL,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Open',
    author_agent_id TEXT REFERENCES agents(id),
    reviewers TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS activity_events (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    description TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);

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
";

const MIGRATION_002: &str = "
ALTER TABLE merge_requests ADD COLUMN diff_files_changed INTEGER;
ALTER TABLE merge_requests ADD COLUMN diff_insertions INTEGER;
ALTER TABLE merge_requests ADD COLUMN diff_deletions INTEGER;
ALTER TABLE merge_requests ADD COLUMN has_conflicts INTEGER;

CREATE TABLE IF NOT EXISTS review_comments (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    author_agent_id TEXT NOT NULL,
    body TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS reviews (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    reviewer_agent_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    body TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_queue (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL REFERENCES merge_requests(id),
    priority INTEGER NOT NULL DEFAULT 50,
    status TEXT NOT NULL DEFAULT 'Queued',
    enqueued_at INTEGER NOT NULL,
    processed_at INTEGER,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_review_comments_mr ON review_comments(merge_request_id);
CREATE INDEX IF NOT EXISTS idx_reviews_mr ON reviews(merge_request_id);
CREATE INDEX IF NOT EXISTS idx_mq_status ON merge_queue(status);
CREATE INDEX IF NOT EXISTS idx_mq_priority_enqueued ON merge_queue(priority DESC, enqueued_at ASC);
";

const MIGRATION_003: &str = "
CREATE TABLE IF NOT EXISTS agent_commits (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_worktrees (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    task_id TEXT,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_commits_agent ON agent_commits(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_commits_repo ON agent_commits(repository_id);
CREATE INDEX IF NOT EXISTS idx_agent_commits_sha ON agent_commits(commit_sha);
CREATE INDEX IF NOT EXISTS idx_agent_worktrees_agent ON agent_worktrees(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_worktrees_repo ON agent_worktrees(repository_id);
";

const MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_001),
    (2, MIGRATION_002),
    (3, MIGRATION_003),
];

pub fn run(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version    INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    for (version, sql) in MIGRATIONS {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM _migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )?;
        if !exists {
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO _migrations (version, applied_at) VALUES (?1, datetime('now'))",
                [version],
            )?;
            tracing::info!("Applied migration {}", version);
        }
    }
    Ok(())
}
