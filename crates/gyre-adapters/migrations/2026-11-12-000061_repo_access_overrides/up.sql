-- Per-repo access overrides (user-management.md §Repo Access).
--
-- An override caps one user's effective role on one repo at
-- `restricted_role`. The effective role is the more restrictive of the
-- user's workspace membership role and the override, so overrides can
-- restrict but never expand access beyond the workspace role. That
-- invariant is validated at the service layer before persisting; this
-- table stores the already-validated cap.
--
-- Portable SQL: TEXT / BIGINT with IF NOT EXISTS runs on both SQLite and
-- PostgreSQL (the shared embedded migrations dir executes on both backends).

CREATE TABLE IF NOT EXISTS repo_access_overrides (
    id              TEXT   NOT NULL PRIMARY KEY,
    repo_id         TEXT   NOT NULL,
    user_id         TEXT   NOT NULL,
    restricted_role TEXT   NOT NULL,
    created_by      TEXT   NOT NULL,
    created_at      BIGINT NOT NULL,
    CONSTRAINT uq_repo_access_overrides_repo_user UNIQUE (repo_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_repo_access_overrides_repo ON repo_access_overrides (repo_id);
