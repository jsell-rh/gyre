-- Revert TASK-119: drop the repo budget override and compute-minute columns.

ALTER TABLE repositories DROP COLUMN budget_json;

ALTER TABLE budget_usages DROP COLUMN compute_minutes;
