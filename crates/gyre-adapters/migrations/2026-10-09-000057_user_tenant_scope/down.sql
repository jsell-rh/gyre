-- Revert TASK-110 repair F1: drop user tenant scope columns.

DROP INDEX IF EXISTS idx_users_tenant;
ALTER TABLE users DROP COLUMN global_role;
ALTER TABLE users DROP COLUMN tenant_id;
