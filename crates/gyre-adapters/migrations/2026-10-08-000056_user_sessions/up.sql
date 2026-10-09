-- TASK-111: user session management (user-management.md §Session Management).
--
-- One row per authenticated session. Created on successful API-key auth;
-- `token_hash` is the SHA-256 hex of the session token (plaintext never
-- stored). `revoked` sessions are kept for audit until the retention cleanup
-- job deletes rows whose `expires_at` is older than the cutoff.

CREATE TABLE IF NOT EXISTS user_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    ip_address TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    created_at BIGINT NOT NULL,
    last_active_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_user_sessions_token_hash ON user_sessions(token_hash);
CREATE INDEX IF NOT EXISTS idx_user_sessions_expires_at ON user_sessions(expires_at);
