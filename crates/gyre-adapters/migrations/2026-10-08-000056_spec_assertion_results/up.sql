-- Persisted spec assertion results (system-explorer.md §9).
-- One row per assertion, keyed by (repo_id, spec_path, line): the post-push
-- knowledge-graph check replaces any prior rows for the spec so the stored
-- set always reflects the latest push's check.
CREATE TABLE spec_assertion_results (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    spec_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    assertion_type TEXT NOT NULL,
    assertion_text TEXT NOT NULL,
    params_json TEXT NOT NULL,
    passed BOOLEAN NOT NULL,
    explanation TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    checked_at BIGINT NOT NULL
);

CREATE INDEX idx_spec_assertion_results_repo_spec
    ON spec_assertion_results (repo_id, spec_path);
