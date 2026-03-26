-- M38 Message Bus Phase 2: messages table for Directed and Event tier storage.
-- Broadcast messages are never stored (workspace_id NOT NULL enforces this).

CREATE TABLE messages (
    id            TEXT    NOT NULL PRIMARY KEY,
    tenant_id     TEXT    NOT NULL,
    from_type     TEXT    NOT NULL,          -- 'server', 'agent', 'user'
    from_id       TEXT,                      -- NULL for server origin
    workspace_id  TEXT    NOT NULL,          -- always present; Broadcast messages not stored
    to_type       TEXT    NOT NULL,          -- 'agent', 'workspace'
    to_id         TEXT,                      -- agent_id or workspace_id
    kind          TEXT    NOT NULL,
    payload       TEXT,                      -- JSON
    created_at    INTEGER NOT NULL,          -- Unix epoch MILLISECONDS
    signature     TEXT,
    key_id        TEXT,
    acknowledged  INTEGER NOT NULL DEFAULT 0,
    ack_reason    TEXT                       -- NULL, 'explicit', 'agent_completed', 'agent_orphaned'
);

-- Agent inbox lookup: unacked messages for a specific agent
CREATE INDEX idx_messages_inbox ON messages (to_type, to_id, acknowledged)
    WHERE to_type = 'agent' AND acknowledged = 0;

-- Workspace event history (newest first)
CREATE INDEX idx_messages_workspace ON messages (workspace_id, created_at DESC);

-- Kind-filtered workspace queries
CREATE INDEX idx_messages_kind ON messages (workspace_id, kind, created_at DESC);

-- Event expiry (non-agent messages only)
CREATE INDEX idx_messages_expiry ON messages (created_at) WHERE to_type != 'agent';

-- Admin cross-tenant queries
CREATE INDEX idx_messages_tenant ON messages (tenant_id);
