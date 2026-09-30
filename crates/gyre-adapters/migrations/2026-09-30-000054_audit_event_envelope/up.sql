-- TASK-102: Expand audit_events to the spec-compliant envelope
-- (observability.md §Audit Event Schema). 14 fields; agent_id becomes
-- nullable (server-initiated events carry none); path/pid are dropped —
-- their per-row values are folded into the detail JSON payload; details
-- renamed to detail.
--
-- SQLite cannot relax the NOT NULL on agent_id with ALTER TABLE, so the
-- table is rebuilt (same pattern as 000015_ralph_step_removal). Existing
-- rows keep their data: path/pid move into detail (merge-patch semantics
-- drop the key when the source value was NULL).
CREATE TABLE audit_events_new (
    id TEXT PRIMARY KEY NOT NULL,
    event_type TEXT NOT NULL,
    agent_id TEXT,
    user_id TEXT,
    session_id TEXT,
    workspace_id TEXT,
    repo_id TEXT,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    outcome TEXT NOT NULL DEFAULT 'success',
    detail TEXT NOT NULL DEFAULT '{}',
    source_ip TEXT,
    user_agent TEXT,
    timestamp BIGINT NOT NULL
);

INSERT INTO audit_events_new
    (id, event_type, agent_id, resource_type, resource_id, outcome, detail, timestamp)
SELECT
    id,
    event_type,
    agent_id,
    'agent',
    agent_id,
    'success',
    json_patch(details, json_object('path', path, 'pid', pid)),
    timestamp
FROM audit_events;

DROP TABLE audit_events;
ALTER TABLE audit_events_new RENAME TO audit_events;

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_agent_id ON audit_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_events(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_workspace_id ON audit_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_audit_user_id ON audit_events(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_resource_type ON audit_events(resource_type);
CREATE INDEX IF NOT EXISTS idx_audit_outcome ON audit_events(outcome);
