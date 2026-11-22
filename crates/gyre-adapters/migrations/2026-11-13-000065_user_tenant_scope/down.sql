-- Renumber note: renumbered to 20261113000065 at the rebase onto main
-- head f764bc00 (see up.sql).

-- Revert TASK-110 repair F1: drop the tenant-scope index.
--
-- The tenant_id and global_role columns themselves belong to
-- 2026-10-08-000058_user_entity_profile_fields (see the up.sql rebase
-- note): rollback order is reverse-version, so this down runs first and
-- must only drop what this migration added.

DROP INDEX IF EXISTS idx_users_tenant;
