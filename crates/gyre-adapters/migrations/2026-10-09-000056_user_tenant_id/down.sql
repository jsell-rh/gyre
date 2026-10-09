-- Revert TASK-099 F1: drop the user tenant binding.

ALTER TABLE users DROP COLUMN tenant_id;
