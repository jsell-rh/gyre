-- Secrets table (platform-model.md §7 Secrets Delivery)

-- Secret metadata + AES-256-GCM encrypted value. `encrypted_value` and
-- `nonce` are BLOBs written only by the adapter layer; plaintext never
-- touches this table. `scope_id` holds the tenant_id, workspace_id, repo_id,
-- or task_id depending on `scope`.
CREATE TABLE IF NOT EXISTS secrets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    scope TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    secret_type TEXT NOT NULL,
    encrypted_value BLOB NOT NULL,
    nonce BLOB NOT NULL,
    created_by TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    expires_at BIGINT,
    last_rotated_at BIGINT,
    tenant_id TEXT NOT NULL,
    UNIQUE (tenant_id, scope, scope_id, name)
);

-- Scope-based resolution: resolve_for_agent queries by (scope, scope_id)
-- for each of the tenant/workspace/repo/task scopes, filtered by tenant_id.
CREATE INDEX IF NOT EXISTS idx_secrets_scope
    ON secrets (scope, scope_id, tenant_id);
