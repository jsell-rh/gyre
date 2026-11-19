-- TASK-273 (admin-panel.md §Background Jobs: OAuth state cleanup): durable
-- OAuth 2.0 authorization-request state rows for admin-initiated Claude Max
-- account enrollment. Rows carry the initiating admin and tenant (derived
-- server-side at initiation), the PKCE verifier, and an expiry consumed by
-- the oauth_state_cleanup job.

CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT,
    initiated_by TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0,
    provider_config TEXT NOT NULL
);

CREATE INDEX idx_oauth_states_expires ON oauth_states (expires_at);
