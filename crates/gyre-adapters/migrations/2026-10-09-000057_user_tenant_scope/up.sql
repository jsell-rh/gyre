-- TASK-110 repair F1 (user-management.md §User Entity, §Workspace
-- Invitation Flow): the domain User carries tenant_id and global_role,
-- but the users table has no columns for them — the DB adapters
-- silently dropped both on every create/read, so in GYRE_DATABASE_URL
-- deployments (the documented production mode) the workspace invitation
-- flow rejected every invitee with 403 "user has no tenant scope" and
-- the invited GlobalRole was lost.
--
-- Rebase note (merge of c5deca801, task-105/task-120 lineage): the
-- tenant_id and global_role columns are now created by
-- 2026-10-08-000058_user_entity_profile_fields, whose Diesel version
-- (20261008000058) sorts BEFORE this migration's (20261009000057), so
-- it always runs first on a fresh database — re-adding the columns here
-- would abort with "duplicate column name" and prevent startup. This
-- migration keeps only the tenant-scope index it uniquely contributes.
-- Databases that already ran the pre-rebase version of this migration
-- are unaffected: Diesel records versions, not file contents, and their
-- columns/index already exist.

CREATE INDEX idx_users_tenant ON users (tenant_id);
