-- M13.7: Spawn log and revoked tokens for atomic agent operations

CREATE TABLE IF NOT EXISTS spawn_log (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    step TEXT NOT NULL,
    status TEXT NOT NULL,
    detail TEXT,
    occurred_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS revoked_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    revoked_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_spawn_log_agent ON spawn_log(agent_id);
CREATE INDEX IF NOT EXISTS idx_spawn_log_occurred ON spawn_log(occurred_at);
CREATE INDEX IF NOT EXISTS idx_revoked_tokens_agent ON revoked_tokens(agent_id);
