-- Revert TASK-118: drop the per-repo agent limit columns.

ALTER TABLE repositories DROP COLUMN max_agents;
ALTER TABLE repositories DROP COLUMN max_agent_iterations;
