-- Revert TASK-124: drop the user-deactivation column.

ALTER TABLE users DROP COLUMN deactivated_at;
