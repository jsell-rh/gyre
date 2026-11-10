-- TASK-102: Expand audit_events to the spec-compliant envelope
-- (observability.md §Audit Event Schema). 14 fields; agent_id becomes
-- nullable (server-initiated events carry none); path/pid are dropped —
-- their per-row values are folded into the detail JSON payload; details
-- renamed to detail.
--
-- SQLite cannot relax the NOT NULL on agent_id with ALTER TABLE, so the
-- table is rebuilt (same pattern as 000015_ralph_step_removal). Existing
-- rows keep their data: path/pid move into detail.
--
-- PORTABILITY (task-102 F1): this migrations directory is embedded once
-- and executed by BOTH storage backends (sqlite/mod.rs and
-- postgres/mod.rs embed the same dir), so the fold below uses only SQL
-- that runs on SQLite AND PostgreSQL. The previous revision folded with
-- SQLite-only JSON1 functions (json_patch/json_object), which aborted
-- every PostgreSQL deployment at startup, at migration time. The fold is
-- now CASE WHEN + string concatenation.
--
-- Fold semantics (RFC 7396 merge-patch, the same behavior the JSON1
-- form had): a NULL path/pid contributes no key; an object details
-- keeps its keys with path/pid appended before the closing brace; a
-- non-object details (never written by in-repo code — every writer
-- stored serde_json::to_string output, so an object renders as braced
-- canonical text — but conceivable in rows pushed through the
-- pre-envelope HTTP API with a scalar details body) is replaced by the
-- path/pid object, which is what merge-patch defines for a non-object
-- target.
--
-- The path value is JSON-escaped for backslash, double quote and the
-- whitespace control characters that can legally occur in file paths;
-- string literals below rely on standard_conforming_strings (the
-- PostgreSQL default since 9.1) so '\' is one backslash and '\n' is
-- backslash + n, exactly as in SQLite. Remaining C0 control characters
-- cannot occur in paths written by any in-repo emitter; a row carrying
-- one would degrade on read to the adapters' existing corrupt-JSON
-- fallback (empty detail object) rather than corrupt other rows.
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
    CASE
        -- Nothing to fold: keep the legacy payload verbatim.
        WHEN path IS NULL AND pid IS NULL THEN details
        ELSE
            CASE
                -- Object payload: open it up — drop the closing brace and
                -- keep a separating comma unless the object was empty ...
                WHEN substr(details, 1, 1) = '{'
                     AND substr(details, length(details), 1) = '}'
                THEN substr(details, 1, length(details) - 1)
                     || CASE WHEN length(details) > 2 THEN ',' ELSE '' END
                -- ... non-object payload: merge-patch replaces the target.
                ELSE '{'
            END
            -- ... append the folded keys (a NULL source value adds no
            -- key), JSON-escaping the path string ...
            || CASE
                WHEN path IS NULL THEN ''
                ELSE '"path":"'
                     || replace(replace(replace(replace(replace(
                            path,
                            '\', '\\'),
                            '"', '\"'),
                            '
', '\n'),
                            '', '\r'),
                            '	', '\t')
                     || '"'
            END
            || CASE WHEN path IS NULL OR pid IS NULL THEN '' ELSE ',' END
            || CASE WHEN pid IS NULL THEN '' ELSE '"pid":' || CAST(pid AS TEXT) END
            -- ... and close the object.
            || '}'
    END,
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
