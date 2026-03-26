-- HSI §2: Replace the old notifications table with the new schema.
--
-- The old schema (M22.8) used: read BOOL, read_at, entity_type, entity_id,
-- priority TEXT enum (Low/Medium/High/Urgent), no workspace/tenant fields.
-- The new schema uses: resolved_at, dismissed_at, entity_ref, priority INTEGER (1-10),
-- workspace_id, tenant_id, repo_id — matching the HSI spec exactly.

DROP TABLE IF EXISTS notifications;

CREATE TABLE notifications (
    id          TEXT    PRIMARY KEY NOT NULL,
    workspace_id TEXT   NOT NULL,
    user_id      TEXT   NOT NULL,
    notification_type TEXT NOT NULL,
    priority     INTEGER NOT NULL,   -- 1 (highest) to 10 (lowest) per HSI §8
    title        TEXT   NOT NULL,
    body         TEXT,               -- optional JSON payload with type-specific data
    entity_ref   TEXT,               -- optional: spec_path, agent_id, mr_id, etc.
    repo_id      TEXT,               -- optional: set for repo-scope Inbox filtering
    resolved_at  INTEGER,            -- epoch seconds; NULL if not yet resolved
    dismissed_at INTEGER,            -- epoch seconds; NULL if not dismissed
    created_at   INTEGER NOT NULL,
    tenant_id    TEXT    NOT NULL
);

CREATE INDEX idx_notifications_user_ws
    ON notifications (user_id, workspace_id, resolved_at);
