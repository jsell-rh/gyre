-- Revert TASK-095: drop the post-merge gate columns.
-- SQLite >= 3.35 supports DROP COLUMN; the bundled system SQLite qualifies.

ALTER TABLE quality_gates DROP COLUMN gate_phase;
ALTER TABLE quality_gates DROP COLUMN timeout_secs;
