-- TASK-110 (user-management.md §Tenant-Level User Onboarding,
-- §Workspace Invitation Flow, §Invitation Expiry): tenant and workspace
-- invitation tables.
--
-- tenant_invitations onboards NEW users into the tenant by email (magic
-- link); workspace_invitations grants EXISTING tenant users workspace
-- access. Both store only the SHA-256 hash of the invitation token and
-- share the status lifecycle Pending/Accepted/Declined/Expired/Revoked.
-- Expired rows are marked by a background job, never deleted (audit).
--
-- Unique constraints enforce one PENDING invitation per (tenant, email) /
-- (workspace, user); the partial index keeps re-invite after terminal
-- states (Revoked/Declined/Expired) possible with a fresh token.

CREATE TABLE tenant_invitations (
    id               TEXT PRIMARY KEY NOT NULL,
    tenant_id        TEXT NOT NULL,
    email            TEXT NOT NULL,
    invited_by       TEXT NOT NULL,
    role             TEXT NOT NULL,
    workspace_ids    TEXT NOT NULL,   -- JSON array of workspace ids
    workspace_roles  TEXT NOT NULL,   -- JSON array of workspace roles
    status           TEXT NOT NULL,
    token_hash       TEXT NOT NULL,
    expires_at       INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    accepted_at      INTEGER
);

CREATE UNIQUE INDEX idx_tenant_invitations_token
    ON tenant_invitations (token_hash);

CREATE INDEX idx_tenant_invitations_tenant
    ON tenant_invitations (tenant_id, status);

CREATE TABLE workspace_invitations (
    id               TEXT PRIMARY KEY NOT NULL,
    tenant_id        TEXT NOT NULL,
    workspace_id     TEXT NOT NULL,
    user_id          TEXT NOT NULL,
    invited_by       TEXT NOT NULL,
    role             TEXT NOT NULL,
    status           TEXT NOT NULL,
    token_hash       TEXT NOT NULL,
    expires_at       INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    accepted_at      INTEGER
);

CREATE UNIQUE INDEX idx_workspace_invitations_token
    ON workspace_invitations (token_hash);

CREATE INDEX idx_workspace_invitations_workspace
    ON workspace_invitations (workspace_id, status);

CREATE INDEX idx_workspace_invitations_user
    ON workspace_invitations (user_id, status);
