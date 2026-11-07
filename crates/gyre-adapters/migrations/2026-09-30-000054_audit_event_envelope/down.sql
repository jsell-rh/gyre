-- Rollback TASK-102: restore the pre-envelope audit_events shape
-- (id, agent_id NOT NULL, event_type, path, details, pid, timestamp).
--
-- PORTABILITY (task-102 F1): the previous rollback used SQLite-only
-- JSON1 functions (json_extract/json_remove) to pull the folded keys
-- back out of detail; like the up migration, that broke PostgreSQL
-- deployments at startup. Pulling a key back out of a JSON text cannot
-- be done in SQL that is portable across both engines (they share no
-- substring-search function, and JSON unescaping is not expressible
-- with replace()), so this rollback keeps the envelope's detail payload
-- verbatim in `details` and restores path/pid as NULL: no data is lost
-- — the folded "path"/"pid" keys remain readable inside details — but
-- the legacy columns come back empty. The forward fold is lossless;
-- the rollback is shape-restoring, not value-restoring, the same
-- one-way character the previous revision had in reverse (it collapsed
-- NULL agent_id to '').
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
    NULL,
    detail,
    NULL,
    timestamp
FROM audit_events;

DROP TABLE audit_events;
ALTER TABLE audit_events_old RENAME TO audit_events;

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_agent_id ON audit_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_events(event_type);
