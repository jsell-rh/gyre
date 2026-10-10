-- TASK-110 repair F1 (user-management.md §User Entity, §Workspace
-- Invitation Flow): the domain User carries tenant_id and global_role,
-- but the users table has no columns for them — the DB adapters
-- silently dropped both on every create/read, so in GYRE_DATABASE_URL
-- deployments (the documented production mode) the workspace invitation
-- flow rejected every invitee with 403 "user has no tenant scope" and
-- the invited GlobalRole was lost.
--
-- Columns are nullable/defaulted so the migration is idempotent-safe on
-- populated tables: pre-existing users are tenant-less Member accounts
-- exactly as they already behaved.

ALTER TABLE users ADD COLUMN tenant_id TEXT;
ALTER TABLE users ADD COLUMN global_role TEXT NOT NULL DEFAULT 'Member';

CREATE INDEX idx_users_tenant ON users (tenant_id);
