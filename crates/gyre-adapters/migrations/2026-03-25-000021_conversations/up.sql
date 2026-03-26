-- HSI §5: Conversation-to-code provenance tables.
-- Stores agent conversation blobs and maps conversation turns to commits.

CREATE TABLE conversations (
    sha TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    blob BLOB,                   -- NULL if stored on disk (>1MB uncompressed)
    file_path TEXT,              -- set if stored on disk; SHA used as filename
    created_at INTEGER NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE TABLE turn_commit_links (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    turn_number INTEGER NOT NULL,
    commit_sha TEXT NOT NULL,
    files_changed TEXT NOT NULL, -- JSON array of file paths
    conversation_sha TEXT,       -- NULL until back-filled at conversation upload
    timestamp INTEGER NOT NULL,
    tenant_id TEXT NOT NULL
);

CREATE INDEX idx_turn_links_agent ON turn_commit_links (agent_id);
CREATE INDEX idx_turn_links_conversation ON turn_commit_links (conversation_sha);
