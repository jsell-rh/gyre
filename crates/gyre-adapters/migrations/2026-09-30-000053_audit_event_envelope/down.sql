-- Rollback TASK-102: restore the pre-envelope audit_events shape
-- (id, agent_id NOT NULL, event_type, path, details, pid, timestamp).
-- detail's "path"/"pid" keys, where present, restore the old columns;
-- the rest of the envelope fields are dropped.
CREATE TABLE audit_events_old (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    path TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    pid INTEGER,
    timestamp BIGINT NOT NULL
);

INSERT INTO audit_events_old (id, agent_id, event_type, path, details, pid, timestamp)
SELECT
    id,
    COALESCE(agent_id, ''),
    event_type,
    json_extract(detail, '$.path'),
    json_remove(detail, '$.path', '$.pid'),
    json_extract(detail, '$.pid'),
    timestamp
FROM audit_events;

DROP TABLE audit_events;
ALTER TABLE audit_events_old RENAME TO audit_events;

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_agent_id ON audit_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_events(event_type);
