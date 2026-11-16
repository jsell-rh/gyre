-- Revert TASK-188: drop the task deployment generation counter.

ALTER TABLE tasks DROP COLUMN generation;
