-- Revert TASK-099 F2: drop the agent persona binding.

ALTER TABLE agents DROP COLUMN persona_id;
