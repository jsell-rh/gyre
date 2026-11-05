-- Revert TASK-168: drop the gate position column.
-- SQLite >= 3.35 supports DROP COLUMN; the bundled system SQLite qualifies.

ALTER TABLE quality_gates DROP COLUMN position;
