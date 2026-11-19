-- TASK-273 (admin-panel.md §Background Jobs): durable job-run history.
--
-- The in-memory registry history (HashMap<String, VecDeque<JobRun>> in
-- jobs.rs, capped at 50) orphans every run on restart; this table is the
-- authoritative store behind the new JobRunRepository port. `logs` holds
-- the per-run captured log lines as a JSON array of strings.
--
-- Portability: plain CREATE TABLE with INTEGER booleans and no
-- dialect-only syntax, runnable by both the SQLite and PostgreSQL
-- embedded-migration runners.

CREATE TABLE job_runs (
    id TEXT PRIMARY KEY NOT NULL,
    job_name TEXT NOT NULL,
    started_at BIGINT NOT NULL,
    finished_at BIGINT,
    status TEXT NOT NULL,
    error TEXT,
    logs TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_job_runs_job_name_started ON job_runs (job_name, started_at DESC);
