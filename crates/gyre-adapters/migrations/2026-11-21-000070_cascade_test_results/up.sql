-- Durable cascade test results (dependency-graph.md §Cascade Testing,
-- task-280).
--
-- One row per cascade test task: the merge processor's
-- report_cascade_test_result persists the outcome (pass/fail + failure
-- details) when the agent executing the cascade-test task completes or
-- fails. The UNIQUE constraint on cascade_task_id enforces the port
-- contract (one result per cascade run — re-reporting replaces the row),
-- and the table makes cascade outcomes durable across server restarts
-- instead of living only as one-shot workspace events.
--
-- Portability: plain CREATE TABLE / CREATE INDEX with TEXT, BOOLEAN, and
-- BIGINT types — runnable by both the SQLite and PostgreSQL embedded
-- migration runners (same shape as 000056).

CREATE TABLE cascade_test_results (
    id TEXT PRIMARY KEY,
    dependent_repo_id TEXT NOT NULL,
    merged_repo_id TEXT NOT NULL,
    merge_commit_sha TEXT NOT NULL,
    cascade_task_id TEXT NOT NULL UNIQUE,
    passed BOOLEAN NOT NULL,
    failure_details TEXT,
    recorded_at BIGINT NOT NULL
);

CREATE INDEX idx_cascade_test_results_dependent_repo
    ON cascade_test_results (dependent_repo_id);

CREATE INDEX idx_cascade_test_results_merged_repo
    ON cascade_test_results (merged_repo_id);
