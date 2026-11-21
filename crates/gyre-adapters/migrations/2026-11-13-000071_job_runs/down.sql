-- Revert TASK-273: drop the durable job-run history table.

DROP TABLE IF EXISTS job_runs;
