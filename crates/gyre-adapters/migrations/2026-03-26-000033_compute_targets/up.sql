CREATE TABLE compute_targets (
    id TEXT NOT NULL PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    target_type TEXT NOT NULL CHECK(target_type IN ('Container', 'Ssh', 'Kubernetes')),
    config TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(tenant_id, name)
);

ALTER TABLE workspaces ADD COLUMN compute_target_id TEXT REFERENCES compute_targets(id);
