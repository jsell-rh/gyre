-- User workspace state: per-user, per-workspace last_seen_at tracking.
-- No tenant_id: workspace_id is globally unique, providing structural isolation.
-- This table is internal-only (no REST endpoint) — accessed by middleware and briefing handler.
CREATE TABLE user_workspace_state (
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    last_seen_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, workspace_id)
);
