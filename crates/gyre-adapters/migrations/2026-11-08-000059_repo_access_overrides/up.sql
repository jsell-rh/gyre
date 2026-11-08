-- TASK-121: Per-repo access overrides (user-management.md §Repo Access).
--
-- Repo access is inherited from workspace membership by default; an override
-- restricts a member's effective role on one repo to the more restrictive of
-- their workspace role and the override role. The (repo_id, user_id) pair is
-- unique: one override per member per repo — a second attempt must fail so a
-- stale restriction can never be shadowed by a duplicate row (the port
-- documents "Fails if ... already exists" and every adapter enforces it).
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement below is standard SQL — no dialect-only
-- constraint syntax, no INSERT OR IGNORE, no ::type casts.

CREATE TABLE repo_access_overrides (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT uq_repo_access_overrides_repo_user UNIQUE (repo_id, user_id)
);

CREATE INDEX idx_repo_access_overrides_repo_id ON repo_access_overrides (repo_id);
