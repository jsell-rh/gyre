-- Durable preview environments (activity-dashboard.md §"Try it" links).
--
-- One row per ephemeral environment provisioned from a merged merge
-- request. The row is the durable statement of "exactly this version of
-- the code ran for this user": the resolved commit SHA, the mesh peer
-- binding and the lifecycle status (provisioning → active →
-- expired/failed) all live here, so environments survive a server restart
-- and the TTL expiry job can reclaim processes/peers whose provisioning
-- server died mid-flight.
--
-- Portability: plain CREATE TABLE with TEXT/INTEGER/BOOLEAN columns and a
-- separate index — runnable by both the SQLite and PostgreSQL
-- embedded-migration runners (same shape as 000056/000068).

CREATE TABLE preview_environments (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    repo_id TEXT NOT NULL,
    mr_id TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    status TEXT NOT NULL,
    mesh_ip TEXT,
    peer_id TEXT,
    agent_id TEXT NOT NULL,
    compute_target TEXT NOT NULL,
    image TEXT,
    data_dir TEXT NOT NULL,
    port INTEGER NOT NULL,
    process_handle TEXT,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    ended_at BIGINT,
    end_reason TEXT
);

CREATE INDEX idx_preview_environments_mr
    ON preview_environments (mr_id);
CREATE INDEX idx_preview_environments_workspace
    ON preview_environments (workspace_id);
-- The TTL expiry job scans for live environments past their deadline.
CREATE INDEX idx_preview_environments_expires
    ON preview_environments (expires_at);
