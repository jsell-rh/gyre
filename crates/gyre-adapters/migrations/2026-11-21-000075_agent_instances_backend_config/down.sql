-- Revert TASK-273 review: drop the kill-reconstruction column.

ALTER TABLE agent_instances DROP COLUMN backend_config;
