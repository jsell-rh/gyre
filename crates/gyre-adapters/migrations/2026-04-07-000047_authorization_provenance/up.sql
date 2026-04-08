-- Authorization provenance tables (authorization-provenance.md §5.3–5.4)

-- §1.1 Trust anchors — registered identity issuers per tenant
CREATE TABLE IF NOT EXISTS trust_anchors (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    issuer TEXT NOT NULL,
    jwks_uri TEXT NOT NULL,
    anchor_type TEXT NOT NULL,
    constraints_json TEXT NOT NULL DEFAULT '[]',
    created_at BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, id)
);

-- §2.3 Key bindings — ephemeral Ed25519 keys bound to user/agent identity
CREATE TABLE IF NOT EXISTS key_bindings (
    id TEXT PRIMARY KEY NOT NULL,
    user_identity TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    public_key BLOB NOT NULL,
    issuer TEXT NOT NULL,
    trust_anchor_id TEXT NOT NULL,
    issued_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    user_signature BLOB NOT NULL,
    platform_countersign BLOB NOT NULL,
    revoked_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_key_bindings_tenant_identity
    ON key_bindings (tenant_id, user_identity);
CREATE INDEX IF NOT EXISTS idx_key_bindings_tenant_pubkey
    ON key_bindings (tenant_id, public_key);

-- §5.1 Chain attestations — content-addressable attestation chain nodes
CREATE TABLE IF NOT EXISTS chain_attestations (
    id TEXT PRIMARY KEY NOT NULL,
    input_type TEXT NOT NULL,
    input_json TEXT NOT NULL,
    output_json TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    parent_ref TEXT,
    chain_depth INTEGER NOT NULL DEFAULT 0,
    workspace_id TEXT NOT NULL,
    repo_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    tenant_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chain_attestations_task
    ON chain_attestations (task_id);
CREATE INDEX IF NOT EXISTS idx_chain_attestations_commit
    ON chain_attestations (commit_sha);
CREATE INDEX IF NOT EXISTS idx_chain_attestations_repo_time
    ON chain_attestations (repo_id, created_at);
CREATE INDEX IF NOT EXISTS idx_chain_attestations_parent
    ON chain_attestations (parent_ref);
CREATE INDEX IF NOT EXISTS idx_chain_attestations_workspace
    ON chain_attestations (workspace_id);
