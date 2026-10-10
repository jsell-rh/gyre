-- Migration 000056: Per-repo spec lifecycle configuration
-- spec-lifecycle.md §Configuration — externalizes the hardcoded watched/
-- ignored path lists and task priorities from git_http.rs into per-repo
-- config. Absent rows mean defaults (handled in domain Default impl).
CREATE TABLE IF NOT EXISTS spec_lifecycle_configs (
    repo_id TEXT PRIMARY KEY NOT NULL,
    config TEXT NOT NULL
);
