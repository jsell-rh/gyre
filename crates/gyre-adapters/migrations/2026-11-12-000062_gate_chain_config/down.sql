-- task-135: drop gate-chain ordering and review attribution columns.
-- SQLite cannot DROP COLUMN before 3.35; both supported SQLite (>= 3.35)
-- and PostgreSQL handle plain DROP COLUMN. No defaults or indexes involved.

ALTER TABLE reviews DROP COLUMN gate_id;
ALTER TABLE reviews DROP COLUMN reviewer_persona;

ALTER TABLE quality_gates DROP COLUMN position;
