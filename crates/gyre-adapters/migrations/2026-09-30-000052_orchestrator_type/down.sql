-- Revert TASK-093: drop the orchestrator columns. SQLite 3.35+ supports DROP COLUMN.
ALTER TABLE agents DROP COLUMN restart_on_failure;
ALTER TABLE agents DROP COLUMN repo_id;
ALTER TABLE agents DROP COLUMN orchestrator_type;
