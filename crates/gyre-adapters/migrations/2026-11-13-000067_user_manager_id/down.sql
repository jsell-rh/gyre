-- Revert TASK-265: drop the SCIM enterprise-extension manager column.

ALTER TABLE users DROP COLUMN manager_id;
