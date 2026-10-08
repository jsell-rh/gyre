-- task-163: persistent storage for breaking changes and per-workspace
-- dependency enforcement policies (dependency-graph.md §Enforcement Policies).

CREATE TABLE IF NOT EXISTS breaking_changes (
    id TEXT NOT NULL PRIMARY KEY,
    dependency_edge_id TEXT NOT NULL,
    source_repo_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    description TEXT NOT NULL,
    detected_at BIGINT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    acknowledged_by TEXT,
    acknowledged_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_breaking_changes_edge ON breaking_changes (dependency_edge_id);
CREATE INDEX IF NOT EXISTS idx_breaking_changes_source_repo ON breaking_changes (source_repo_id);
CREATE INDEX IF NOT EXISTS idx_breaking_changes_unacknowledged
    ON breaking_changes (acknowledged, detected_at);

CREATE TABLE IF NOT EXISTS dependency_policies (
    workspace_id TEXT NOT NULL PRIMARY KEY,
    breaking_change_behavior TEXT NOT NULL DEFAULT 'warn',
    max_version_drift INTEGER NOT NULL DEFAULT 3,
    stale_dependency_alert_days INTEGER NOT NULL DEFAULT 30,
    require_cascade_tests INTEGER NOT NULL DEFAULT 1,
    auto_create_update_tasks INTEGER NOT NULL DEFAULT 1,
    updated_at BIGINT NOT NULL
);
