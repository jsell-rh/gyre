-- task-135: drop review attribution columns.
-- SQLite cannot DROP COLUMN before 3.35; both supported SQLite (>= 3.35)
-- and PostgreSQL handle plain DROP COLUMN. No defaults or indexes involved.
--
-- The quality_gates.position column is owned by the task-168 migration
-- 2026-10-08-000062_gate_position; its own down.sql reverts it (this
-- migration's down runs first, while the column still exists for the
-- earlier version's revert).

ALTER TABLE reviews DROP COLUMN gate_id;
ALTER TABLE reviews DROP COLUMN reviewer_persona;
